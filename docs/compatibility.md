# 互換性

| プラグイン | aviutl2-rs | 最小 AviUtl2 | 実機確認済み | Rust | API | ビルド経路 |
|---|---|---|---|---|---|---|
| 0.0.1（Phase 0） | 0.41.0 | 2.1.2（SDK wrapperの宣言値） | AviUtl2 2.1.2 / Windows 11、Windows Server 2022 | 1.88.0 | Phase 0 probeのみ | Linux Dockerクロスビルド・Windows native buildともロード確認済み |

Windows 11では、プロジェクト識別子を現名称へ統一する前のLinux Docker
クロスビルド成果物を確認しました。現名称のWindows native build成果物は、
GitHub-hosted Windows runnerでロード、health、read section、正常終了を
確認しています。
