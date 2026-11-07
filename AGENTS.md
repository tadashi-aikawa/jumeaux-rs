# AGENTS

## コミットメッセージ

Conventional Commits形式で日本語で書く。

### フォーマット

```text
<type>(<scope>): <description>
```

- `type`: `feat`, `fix`, `refactor`, `style`, `docs`, `chore`, `build`, `ci`, `test`
  - 破壊的変更がある場合は `feat!` のように `!` を付ける
- `scope`: `deps`, `cli` など機能単位。省略可
- `description`: ユーザー視点で何が変わったかを簡潔に書く

### descriptionの書き方

- ユーザーにとって何が変わるかを書く
- 「〜を追加」「〜を修正」「〜に変更」のように結果を述べる
- 内部的なリファクタリングの場合のみ実装視点で書いてよい

## ユニットテスト実行

```bash
cargo test
```

## E2Eテスト実行

後続タスクで比較ハーネスと実行コマンドを追加する。

## フォーマット・Lint

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```
