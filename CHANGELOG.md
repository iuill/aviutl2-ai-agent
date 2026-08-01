# Changelog

このプロジェクトの利用者向け変更を記録します。形式は
[Keep a Changelog](https://keepachangelog.com/ja/1.1.0/)を参考にし、versionは
[Semantic Versioning](https://semver.org/lang/ja/)に従います。

## [Unreleased]

## [0.1.1] - 2026-08-02

### Changed

- Rust toolchainを1.97.1へ更新し、依存crateとクロスビルド環境を更新
- READMEの導入導線を整理し、GitHub Actionsのビルドcacheを改善

### Fixed

- current frame取得後にAviUtl2を終了するとpluginのrender待機が復帰しない問題を修正
- CLIとMCPのcurrent frame取得を60秒でtimeoutし、呼出元が無期限に待機しないよう修正

## [0.1.0] - 2026-08-01

### Added

- AviUtl2 Plugin SDKを使うloopback HTTP API
- current scene、timeline、object details、current frameの参照
- objectの移動、削除、複製と、text・media objectの作成
- text本文、font、size、位置、色の参照と更新
- Windows x64 CLIとstdio MCP Server

### Security

- loopbackの固定Host検証とOrigin拒否
- SDK accessの直列化、入力検証、mutation後のread-back
- plugin unload前のHTTP workerとrender callbackの終了待機

[Unreleased]: https://github.com/iuill/aviutl2-ai-agent/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/iuill/aviutl2-ai-agent/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/iuill/aviutl2-ai-agent/releases/tag/v0.1.0
