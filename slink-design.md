# slink 初版設計 — macOS

> **Status (2026-09-14): historical design record.**
>
> このファイルは、実装前に合意した初版設計を現在の仕様書と誤認しないための案内です。実装前の設計本文は、実装・README更新直後の不変スナップショット [`ddccbf7`](https://github.com/kkensuke/slink/blob/ddccbf7a8ccb4ebdff0e5463d8a2386724af4c94/slink-design.md) に保存されています。

## 現在の正本

現在の利用方法・コマンド・パス規則・終了コードは次を正とします。

- `README.md` — English user documentation
- `README.ja.md` — 日本語ユーザードキュメント
- `docs/defaults.md` — 初版オプションの既定値と判断理由
- `tests/cli.rs` / `tests/audit_regressions.rs` — 受け入れ条件と回帰テスト
- `.github/workflows/ci.yml` — macOS / Linux 上の継続検証

実装は Rust で完了しており、初版設計で挙げた作成・adopt・check/fix/remove・dry-run・relative・parents・クラッシュ復旧・同時更新保護などは自動テストで検証します。APFS の大文字・小文字を区別する形式と区別しない形式についても、macOS CI が一時 APFS ボリュームを作成して重複配置先の扱いを検証します。

## 初版設計から意図的に未確定のもの

実装前設計でリリース準備時の判断として残した以下は、初版 CLI の機能完成条件には含めません。

- 最低対応 macOS バージョンの製品ポリシー
- GitHub Releases 等でのバイナリ公開方針
- Homebrew 等のパッケージ配布
- コード署名・notarization を含む外部配布手順

これらを決める場合は、履歴上の設計本文を書き換えるのではなく、リリース方針として別途文書化します。
