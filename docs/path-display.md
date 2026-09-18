# パス表示の共通化設計

状態: 実装済み。パス表示の共通化範囲と受け入れ条件を記録する。
設計時の調査対象: main `24d9295`、[PR #27](https://github.com/kkensuke/slink/pull/27) の既存実装 `46130e2`。
先行して入っていた target のホーム省略処理は、本設計の共通表示へ置き換えた。

## 1. 採用方針

**「読みやすく整形するパス」と「元の文字列を取得する値」で処理を分ける。**
human / TSV という出力形式だけでは分けない。

- `display_path(path)`: 表示用にパス表記を整理し、二重引用符・エスケープを付ける。
- `quoted(text)`: 元の文字列を整理せず、二重引用符・エスケープだけを付ける。

ホームディレクトリを `~` に省略する表示処理は全て廃止する。
この変更は「表示のための `~` を生成しない」という意味であり、入力の `~/` 展開や、symlink に文字として記録された `~` は維持する。
相対パスは表示時に絶対化しない。

一般の文章には既存の `display_text()` を使い、エラー全文からパスらしい文字列を探して書き換えない。

## 2. 共通処理を使う範囲

### human

以下のパス欄には全て `display_path()` を使う。link / target という名前で処理を分けない。

| 出力 | 対象パス | 変更する既存の入口 |
|---|---|---|
| list | link・target | `list()` の human 分岐 |
| check の問題詳細 | link・target・expected・actual | `render_diagnosis()`、`target_line()` |
| check の未完了操作 | link・target | `check()` の pending ブロック |
| scan の正常な管理済みリンク | link・実際の target | `print_scan_section()` |
| scan の問題がある管理済みリンク | link・target・expected・actual | 既存の `render_diagnosis()` を再利用 |
| scan の未管理リンク | link・実際の target | `print_unmanaged()` |
| scan の走査エラー | 走査できなかった path | `print_scan_human()` |
| 作成・fix・adopt・remove・unregister | link・target・parent | `MutationOutput::print_result()` |
| 同操作の dry-run・復旧結果 | link・target・parent（ある場合） | 同じ `print_result()` |
| 操作失敗 | 独立した link 欄 | `MutationOutput::failure()` |
| 参照先不一致の操作失敗 | link・expected・actual | `failure()` と既存の `target_line()` |

`push_target()` は既存どおり `target_line()` と健康状態の注記を組み合わせる。
各コマンドの engine 側に表示処理を追加しない。
同じ表示用ファイル内でも、`collect_scan()` の走査・パス処理や、`observed_health()`・`projected_health()`・`projected_key()` の判定処理には適用しない。
健康な scan 行などで「どの target を表示するか」という既存の選択は変更しない。

### TSV とその stderr

| 出力 | フィールド | 処理 | 値の契約 |
|---|---|---|---|
| list stdout | LINK・TARGET | `quoted()` | TOML から読み出した登録文字列をそのまま取得 |
| check stdout | LINK | `display_path()` | 検証後のリンク位置 |
| check stdout | TARGET | `display_path()` | 検証・正規化後の登録 target |
| check stdout | ACTUAL_TARGET | `quoted()` | readlink で取得した文字列。MATCH / MISMATCH とも同じ |
| 管理済み scan stdout | LINK・TARGET | `display_path()` | 走査したリンク位置と、検証・正規化後の登録 target |
| 管理済み scan stdout | ACTUAL_TARGET | `quoted()` | readlink で取得した文字列 |
| 未管理 scan stdout | LINK | `display_path()` | 走査したリンク位置 |
| 未管理 scan stdout | ACTUAL_TARGET | `quoted()` | readlink で取得した文字列 |
| 未管理 scan stdout | TARGET・TARGET_STATE・LINK_STATE | 既存の空欄 | 登録値がないことを表す |
| check / scan stderr の ERROR | 独立した link / path 欄 | `display_path()` | エラーの対象位置 |
| 同 ERROR | reason | `quoted()` | 理由文。パスとして整理しない |
| check stderr の PENDING | link・target | `display_path()` | 復旧操作の対象を案内する表示 |

`PENDING:` は TSV のデータ行ではなく、stderr の案内文である。
現在の target の `{:?}` を `display_path(&p.entry.target)` に置き換え、link と揃える。
操作名の `{:?}` は `Operation` の表示なのでそのまま残す。文の形と出力先も維持する。

TSV の列名・列順・状態コード・行順・stdout / stderr の役割は変更しない。
値がないセルは空欄のままとし、`display_path("")` や `quoted("")` で `""` に置き換えない。
一方、list に登録された空文字は実際の値なので、従来どおり JSON の `""` として返す。

「原文」は TOML の引用方法などのソース表記ではなく、読み出した文字列の値を指す。
原文保持の条件は `JSON decode(quoted(value)) == value`。引用・エスケープを省くことではない。

### 共通のパス整理を使わない範囲

| 対象 | 使用する処理 | 理由 |
|---|---|---|
| 上表の raw な TSV 列 | `quoted()` | `/./`・区切りの数・末尾も文字列として取得できる必要がある |
| human の reason / hint / 見出し等 | 現行の文章用処理 | 文章全体はパスではない |
| main の最上位エラー、anyhow のエラーチェーン | 現行のエラー表示 | 生成済みの文章を部分置換しない |
| 不正入力の引用、TOML のエラー内容 | 現行の原文引用 | 入力誤りを特定する情報 |
| `--config` / `-c` | 引用符なしの絶対パス | `open "$(slink --config)"` 等で利用する |
| 入力・登録値の検証、`paths::normalize()` 等 | 既存の paths 処理 | ファイルシステム上の意味を扱う処理 |
| リンク照合・重複判定・ソート・健康状態の判定 | 元の値と既存処理 | 表示文字列で判定しない |
| 管理ファイル・pending / backup 情報・symlink 自体 | 既存の保存処理 | 表示の整理を保存値へ戻さない |

操作失敗でも、独立した `link:` 相当の欄は共通化し、`reason:` の文章は変更しない。
同じ対象パスが理由文にも出る場合、表記が完全一致することまでは今回の契約にしない。
ホーム省略のための `PathError` / `Diagnostic` 型や human / TSV 切り替え付きエラー型は新設しない。

## 3. 整形の仕様

表示用整理は、渡された文字列だけを使う純粋な処理とする。
HOME・現在ディレクトリ・ファイルの存在・権限・symlink の状態を参照しない。

| 項目 | 規則 |
|---|---|
| ホーム配下の絶対パス | 絶対表記を維持。省略しない |
| 相対パス | 相対表記を維持 |
| 途中の独立した `.` 成分 | 除去 |
| 先頭以外の連続する `/` | 1つにまとめる |
| 末尾の `/` | ディレクトリ要求を示す1つの `/` を残す |
| 末尾の `/.` | `/.` を残す |
| `..` | 位置と個数を保持。ルート直下でも解消しない |
| 先頭の `./` | 相対パスの先頭の `.` 成分を残す |
| 先頭の連続する `/` | 個数ごと保持。`//` と `/` を同一視しない |
| 空文字・単独の `.`・ルート | 空文字・`.`・`/` のまま扱う |
| 名前中の `.`、空白、Unicode | そのまま。trim・Unicode 正規化を行わない |
| バックスラッシュ | Unix のファイル名の文字として扱い、区切りにはしない |
| 引用符・制御文字 | 最後に既存の `quoted()` で JSON エスケープ。DEL / C1 の追加エスケープも維持 |
| 非 UTF-8 の Path | 既存方針どおり `"<non-UTF-8>"` と表示。損失変換による名前の推測はしない |

末尾の「意味を保持する」ことと文字列の完全保持は区別する。
例えば `a///` は `"a/"`、`a/./` は `"a/"` と表示する。
区切りの数や `.` の原文まで必要な列は `quoted()` を使う。

先頭の `//` 等は、今回整理対象にする途中の区切りとは分けて保護する。
`/./` は `"/"`、`/.` は `"/."`、`./` は `"./"` とする。
ルートに末尾区切りを追加して、意図せず `//` を作らない。

既存の `Path::components().collect()` は末尾区切りや `/.` を除くため、そのまま使用しない。
この挙動は [Rust の Path::components の仕様](https://doc.rust-lang.org/std/path/struct.Path.html#method.components) でも確認できる。
`paths::normalize()` はさらに実体を確認して `..` を処理するため、表示には流用しない。

上流の入力処理や registry の検証ですでに整理された表記は復元しない。
「末尾等を保持する」は、表示関数に渡された値についての契約である。

### 入出力例

| 入力文字列 | display_path の返り値 |
|---|---|
| `/Users/kkensuke/.zshenv` | `"/Users/kkensuke/.zshenv"` |
| `/a/./b` | `"/a/b"` |
| `/a//./b` | `"/a/b"` |
| `/a/./b/` | `"/a/b/"` |
| `/a/./b/.` | `"/a/b/."` |
| `/a/b///` | `"/a/b/"` |
| `/a/b/./` | `"/a/b/"` |
| `/a/../b` | `"/a/../b"` |
| `/../b` | `"/../b"` |
| `../a/./b` | `"../a/b"` |
| `./a/./b` | `"./a/b"` |
| `~/a/./b`（文字としての ~） | `"~/a/b"` |
| `//server//a/./b/.` | `"//server/a/b/."` |
| `///a//b` | `"///a/b"` |
| `/` / `//` / `///` | `"/"` / `"//"` / `"///"` |
| `/.` / `/./` | `"/."` / `"/"` |
| `.` / `./` / `././` | `"."` / `"./"` / `"./"` |
| 空文字 | `""` |
| ` a ` | `" a "` |

例えば `../a/./b` は human では `"../a/b"`、TSV の ACTUAL_TARGET では `"../a/./b"`。
引用方法は共通で、整理を行うかだけが異なる。

## 4. 関数と依存の設計

配置は `src/output.rs`。このモジュールと子モジュールの `mutation` で使う内部関数とする。
`src/paths.rs` には移さず、表示文字列をパス操作へ再利用しにくい構成を維持する。

| 関数 | 責務 |
|---|---|
| `clean_display_path(text: &str) -> String` | 上記規則で字面を整理するだけ。引用しない |
| `display_path(path: impl AsRef<Path>) -> String` | Path / PathBuf / str / String を受け、文字列化・整理・引用を組み合わせる |
| `quoted(text: &str) -> String` | 既存の JSON 引用と制御文字エスケープ。内容の意味を解釈しない |
| `display_text(text: &str) -> String` | 既存の human の文章用エスケープ |
| `target_line()` / `push_target()` | パスの共通表示とラベル・注記を組み合わせる |
| `render_diagnosis()` / `MutationOutput` | 既存の診断・操作結果レイアウトを組み立てる |

共通入口のコード骨格:

```rust
fn display_path(path: impl AsRef<Path>) -> String {
    let text = path.as_ref().to_str().unwrap_or("<non-UTF-8>");
    quoted(&clean_display_path(text))
}
```

`clean_display_path()` は、元の UTF-8 文字列を `/` で区切って扱う。
先頭の区切り列、相対パスの先頭 `.`、終端の `/` または `/.` を元の文字列から判定し、
中間の空成分・`.` を除き、`..` と通常の名前は順序どおり残す。
文字列で返すことで、Path の再構築時に末尾情報を失う経路を作らない。

出力形式を指定する引数や `raw: bool` は持たせない。
呼び出し側が上の適用表に従って入口を選ぶ。raw な列にも引用処理は共通利用される。
`quoted()` をパス専用に変更すると raw 列と理由文も整理されるため、責務は拡張しない。

統合で削除した関数は `display_link()`・`quoted_path()` と PR #27 の `display_target()`。
TSV 専用の別のパス整理関数や、link / target 別のラッパーは残さない。

## 5. 実装差分の対応

| ファイル | 変更内容 |
|---|---|
| `src/output.rs` | 共通入口と整理処理を実装。human のパス欄、対象 TSV 列、ERROR / PENDING を移行。旧3関数を削除 |
| `src/output/mutation.rs` | import と link・target・parent・失敗時 link を移行。expected / actual は共通 target_line 経由 |
| `tests/read_output.rs` | list / scan の表示更新、raw 列との境界を検証 |
| `tests/check_output.rs` | ホーム省略を前提にしたテストを置換。引用・末尾・不一致・pending の human 表示 |
| `tests/check_format.rs` | check TSV の TARGET と ACTUAL_TARGET、ERROR / PENDING、固定列の契約 |
| `tests/mutation_output.rs` | 操作・dry-run・復旧・parent・失敗表示の期待値を更新 |
| `tests/registry_reading.rs` | 既存の list TSV 原文保持・無効な登録文字列を一覧できるテストを維持 |
| `tests/redesign.rs` | `file/`・`file/.` の保存値・表示・診断結果を確認 |
| `src/output.rs` 内のテスト | 整形仕様の境界ケースを表形式で検証 |
| `README.md`・`README.ja.md` | human の引用・省略廃止・整理規則・出力例、TSV 各列の契約を同時に更新 |

`src/main.rs`・`src/engine.rs`・`src/paths.rs`・`src/inspect.rs`・`src/registry.rs`・`src/transaction.rs` の実行処理は変更不要。
Cargo の依存追加、CLI の引数追加、保存形式の変更も不要。

README の入力例や設定場所を説明する `~` まで一括置換しない。
更新対象は出力仕様と実際の出力例。
TSV の TARGET の説明には、list は登録原文、check / 管理済み scan は検証後の期待値であることを明記する。

## 6. 受け入れ条件

| 観点 | 実装時の検証 |
|---|---|
| 整理規則 | 上の入出力表を直接検証。`a/./.`、`a/.//`、`./.`、`//./` も境界ケースに加える |
| 冪等性 | `clean(clean(s)) == clean(s)`。引用済みの文字列を再入力する契約ではない |
| 引用・制御文字 | 空白・二重引用符・バックスラッシュ・改行・タブ・DEL・C1・日本語を含むパスを JSON 復号して確認。制御文字による行や列の増加がない |
| 非 UTF-8 | Path の既存フォールバックが panic せず、引用されたプレースホルダーになる |
| human の統一 | 同じ入力の link / target / parent が同じ整形になる。HOME 配下でも新しい `~` が生成されない |
| list の境界 | 手編集した `/./`・重複 `/`・末尾・空文字・文字としての `~` が TSV の JSON 復号後に元と一致。human は整理する |
| check / scan の境界 | symlink を `../a//./b/.` 等で作り、ACTUAL_TARGET は readlink の値と完全一致。表示用 TARGET・LINK は仕様どおり |
| scan の未管理行 | 登録関連の3列は空欄、実際の target は ACTUAL_TARGET に原文で残る |
| ERROR / PENDING | 対象パス欄だけ整理され、理由文はそのまま引用。PENDING の両パスは同じ表示規則 |
| 操作出力 | 通常操作、dry-run、復旧、親作成、TargetMismatch と一般失敗が共通のパス表示になる |
| 診断の意味 | `file` / `file/` / `file/.`、symlink を経由する `..` で状態と終了コードが変わらない。判定は既存の元データで行う |
| 永続データ | list / check / scan・dry-run の前後で registry・既存 symlink・pending 内容が変わらない。実操作は従来の保存・復旧契約を維持 |
| 対象外の契約 | config の無引用絶対パス、CLI の `~/` 展開、literal `~`、通常のエラー原因の情報を既存テストでも確認 |

見た目のパスが一致しても、それを根拠に MATCH や健康状態を再計算しない。
TSV の実データ検証では JSON 復号後の文字列を比較し、Path の等価比較で表記の差を見落とさない。

既存の `tests/config.rs`、`tests/redesign.rs`、`tests/cli.rs`、復旧関連テストも回帰確認に使う。
実装では表示関連のテストを更新・追加し、既存 CI の fmt・clippy・test・release build と macOS の APFS 検証を完了条件とする。

## 7. 実装の履歴

1. 整理処理・共通入口・list の移行と境界テストを反映。
2. 診断・scan・操作結果・対象 TSV / stderr を移行し、対応するテストを反映。
3. 日英 README と本書を更新。既存 CI と原文保持の契約を完了条件として確認する。

既存の Draft PR #27 を継続利用し、履歴は書き換えない。
この設計書を追加した時点で残っていた旧方針のホーム省略は、追加 commit で置き換えた。
最終差分では旧関数・旧表示を前提とするテストの期待値を除いている。
