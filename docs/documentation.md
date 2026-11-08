# ドキュメントの開発と公開

## ローカルで確認する

リポジトリルートで実行する。ビルド環境はuvで管理する。

```sh
uv sync --locked
uv run zensical serve
uv run zensical build --clean
```

- 入力: `docs/site/`
- 設定: `zensical.toml`
- 出力: `site/`
- 設計文書: `docs/compat-spec.md` など。サイトの入力には含めない。

## 採用バージョンと公式資料

Zensical 0.1.0を `pyproject.toml` と `uv.lock` で固定する。

- [公式README](https://github.com/zensical/zensical#readme)
- [0.1.0のリリースノート](https://github.com/zensical/zensical/releases/tag/v0.1.0)
- [設定の形式](https://zensical.org/docs/setup/basics/)
- [MkDocsからの移行](https://zensical.org/docs/compatibility/mkdocs/migration/)
- [GitHub Pagesへの公開](https://zensical.org/docs/publish-your-site/)

公式の移行手順に従い、Markdownのパスとナビゲーションの階層を維持する。
設定は `[project]` をルートとするTOMLを使う。
テーマは移行向けの `classic` を使う。
