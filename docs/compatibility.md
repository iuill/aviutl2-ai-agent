# 互換性

## 対応環境

| プラグイン | aviutl2-rs | 最小AviUtl2 | 実機確認済み | Rust | API | ビルド経路 |
|---|---|---|---|---|---|---|
| 0.0.1 | 0.41.0 | 2.1.2（SDK wrapperの宣言値） | AviUtl2 2.1.2 / Windows Server 2022、2025 | 1.88.0 | read 4種、write 6種 | Windows native build、Linux Docker cross-build |

「最小AviUtl2」はSDK wrapperが宣言する条件であり、このプロジェクトが全機能を実機確認した
version範囲ではありません。WindowsとAviUtl2で確認していない組み合わせを、対応済みとは
扱いません。

## 実機確認

環境、ビルド元、再現手順、観測結果を含む時系列の記録は
[`verification/windows.md`](verification/windows.md)にまとめています。主な確認範囲は
次のとおりです。

- Windows native buildとLinux Docker cross-build成果物のplugin load
- health、status、current scene、timeline、object read
- objectのmove、delete、text create/update、duplicate、media create
- stdio MCP ServerのlifecycleとCodexからのread/write tool利用
- plugin unload、HTTP workerのjoin、port再利用、port競合時の縮退動作

Phase 0のSDK調査記録は [`history/phase0.md`](history/phase0.md)を参照してください。
