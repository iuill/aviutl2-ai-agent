# Changelog

このプロジェクトの利用者向け変更を記録します。形式は
[Keep a Changelog](https://keepachangelog.com/ja/1.1.0/)を参考にし、versionは
[Semantic Versioning](https://semver.org/lang/ja/)に従います。

## [Unreleased]

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

[Unreleased]: https://github.com/iuill/aviutl2-ai-agent/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/iuill/aviutl2-ai-agent/releases/tag/v0.1.0
