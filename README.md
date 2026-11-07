# Jumeaux

Python版 [jumeaux](https://github.com/tadashi-aikawa/jumeaux) のRust再実装。外部IF互換を目指して開発中。

## ビルド

```bash
mise install
cargo build --release
```

バイナリは `target/release/jumeaux` に生成される。

## 互換仕様とテスト

- [互換仕様](docs/compat-spec.md)
- [比較ハーネスとfixtureの実行手順](compat/README.md)
