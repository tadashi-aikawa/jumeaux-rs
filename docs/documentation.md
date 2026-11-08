# ドキュメントの開発と公開

## ローカルで確認する

リポジトリルートで実行する。ビルド環境はuvで管理する。

```sh
uv sync --locked
uv run zensical serve
uv run zensical build --clean --strict
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

## 移植元と変更点

移植元はPython版のコミット `dc276b8e3a0486bd8d4de2c8e3900aa36cbb7a66`。

- `mkdocs/ja/` の利用者向けページを `docs/site/ja/` へ移す。
- `mkdocs/img/`、`mkdocs/css/`、モデル説明の画像は元の形式と内容を保持する。
- HomeとQuickstartはRust版のバイナリ配布向けに書き直す。
- AddonsのPythonソース参照はRustのソースへ切り替える。
- カスタムアドオン、Python transformer、`init addon` は準備中として案内する。
- ConfigurationとReportの並列数、設定の検証、`summary.comparison` を更新する。
- TemplateはMiniJinjaによる互換実装を案内する。
- Notifierの古い廃止予定を、Rust版の互換対応に合わせる。
- 設定例のYAML構文、条件式の引用符、レイヤー名、必須項目の欠落を修正する。
- Python版との差異ページを追加する。
- ReleasesはRust版の0.1.0から始め、Python版の履歴へリンクする。
- Python版へ掲載する移行案内は [原稿](python-notice.md) に分ける。

Markdown拡張は従来の設定を維持する。
絵文字の関数参照は `zensical.extensions.emoji` に切り替える。
snippetsの `base_path` は `docs/site` に合わせ、参照先がなければビルドを失敗させる。
ナビゲーションの見出しにあった装飾用の絵文字は外す。

## GitHub Pagesを有効にする

1. GitHubのリポジトリ設定で `Settings > Pages` を開く。
2. `Build and deployment > Source` を `GitHub Actions` にする。
3. この変更をmainへマージし、pushする。
4. `Documentation` ワークフローの成功と公開URLを確認する。

公開URLの案は `https://tadashi-aikawa.github.io/jumeaux-rs/`。
リポジトリ名を変えた場合は `zensical.toml` の `site_url` も変更する。

`.github/workflows/pages.yaml` がmainへのpushでビルドと公開を行う。
Actionsの画面から手動実行もできる。
PythonとZensicalはサイトのビルドにだけ必要で、Jumeauxの実行には不要。

Pagesの公開元設定とGitHub上でのデプロイは、ローカルでは検証できない。
