# リリース

plugin、CLI、MCP Serverは同じworkspace versionを持つ1セットとして公開します。
crates.ioには公開せず、Windows x64成果物をGitHub Releasesで配布します。HTTP APIの
`apiVersion`は成果物versionとは別に管理します。

## バージョニング

tagはSemVerと一致する `vMAJOR.MINOR.PATCH` 形式です。`1.0.0`より前は、互換性を保つ
修正をpatch、機能追加または契約変更をminorとして扱います。release tagと
`Cargo.toml`のworkspace versionが一致しない場合、workflowは失敗します。

tagはmain上のcommitだけを対象とし、一度pushしたtagを別commitへ付け替えません。
公開後に修正が必要なら次のpatch versionを作ります。

plugin情報、HTTPの`pluginVersion`、CLI/MCPの`--version`はbuild時のworkspace versionを
使用します。tagからversion文字列を生成するのではなく、release workflowがtagとの一致を
検証します。公式release buildではtag対象の`GITHUB_SHA`もbuild argとして埋め込み、
plugin情報に12桁の短縮hashを併記します。HTTPの`pluginVersion`は機械比較しやすいSemVerの
ままです。通常のローカルbuildではcommit hashを省略します。

## Release PR

1. `release/vX.Y.Z` branchをmainから作ります。
2. `Cargo.toml`のworkspace versionを更新し、Cargo.lockを更新します。
3. `CHANGELOG.md`の`Unreleased`を新versionへ移し、tag予定日が確定してからrelease日を記録します。
4. `docs/compatibility.md`など、現行versionを示す文書を更新します。
5. 通常のRust checkと配布用Docker buildを実行します。
6. SDK依存の挙動を変更した場合は、Windows実機結果も同じPRへ記録します。
7. PRのCIとreviewが完了したらmainへmergeします。

versionだけを変更した後は、lockfileを次で同期できます。

```bash
cargo check --workspace
```

その後の検証では通常どおり`--locked`を使用します。

## TagとGitHub Release

Release PRのmerge後、mainを同期してannotated tagをpushします。

```bash
RELEASE_VERSION=0.1.1
git switch main
git pull --ff-only
git tag -a "v${RELEASE_VERSION}" -m "v${RELEASE_VERSION}"
git push origin "v${RELEASE_VERSION}"
```

`Release` workflowはtag、workspace version、main上のcommitであることを検証してから、
CI cacheを読み込まずに配布用Docker buildを実行します。成功すると次をGitHub Releaseへ
配置します。

- `aviutl2-ai-agent-vX.Y.Z-windows-x64.zip`
- `aviutl2-ai-agent-vX.Y.Z-windows-x64.zip.sha256`

zipにはplugin、CLI、MCP Server、各binaryの`SHA256SUMS`、LICENSE、READMEを含めます。
workflowはzipの外部checksumとzip内binaryのchecksumを公開前にも検証します。外部checksumは
downloadした配布物の完全性を確認する値であり、別のbuildで生成したzipとのbit単位の一致を
保証するものではありません。GitHub Release本文は`CHANGELOG.md`の該当versionから生成します。
開発途中のPR一覧ではなく、利用者に影響する機能、安全性、制約を簡潔に記載します。

workflowが一時的な理由で失敗した場合は同じrunを再実行します。workflowやsourceの修正が
必要な場合は、Releaseが未作成でもtagを削除・付け替えず、原因を修正した次のpatch releaseを
作ります。誤った成果物を公開した場合も同じ扱いです。

## 公開後の確認

```bash
RELEASE_VERSION=0.1.1
gh release view "v${RELEASE_VERSION}"
gh release download "v${RELEASE_VERSION}" \
  --pattern "aviutl2-ai-agent-v${RELEASE_VERSION}-windows-x64*"
sha256sum --check \
  "aviutl2-ai-agent-v${RELEASE_VERSION}-windows-x64.zip.sha256"
```

downloadしたzipを展開し、`SHA256SUMS`を使って3 binaryも確認します。release page、tag、
workspace versionが一致し、最新releaseが意図したversionを指すことを確認して完了です。
