# 互換性

| プラグイン | aviutl2-rs | 最小 AviUtl2 | 実機確認済み | Rust | API | ビルド経路 |
|---|---|---|---|---|---|---|
| 0.0.1（Phase 1開発中） | 0.41.0 | 2.1.2（SDK wrapperの宣言値） | Phase 0成果物のみ: AviUtl2 2.1.2 / Windows 11、Windows Server 2022 | 1.88.0 | `health`、`status`、current scene | Phase 1成果物はWindows未確認 |

Phase 0では、Windows 11上でLinux Dockerクロスビルド成果物を、GitHub-hosted
Windows runner上でWindows native build成果物のロード、health、read section、
正常終了を確認しました。Phase 1のAPIへ置き換えた現ソースのWindows実機確認は
まだ行っていません。
