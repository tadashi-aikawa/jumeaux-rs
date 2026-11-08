# Jumeaux

2つのAPIレスポンスの差分を確認するCLIツールです。
Python版 [Jumeaux](https://github.com/tadashi-aikawa/jumeaux) のRust実装です。

- [ドキュメント](https://tadashi-aikawa.github.io/jumeaux-rs/)
- [Python版からの移行](https://tadashi-aikawa.github.io/jumeaux-rs/ja/getstarted/migration/)
- [互換仕様](docs/compat-spec.md)

## インストール

[GitHub Releases](https://github.com/tadashi-aikawa/jumeaux-rs/releases) からバイナリを取得します。
実行にPythonとRustは不要です。

| 環境 | ターゲット |
| --- | --- |
| macOS Apple Silicon | `aarch64-apple-darwin` |
| macOS Intel | `x86_64-apple-darwin` |
| Linux x86_64 | `x86_64-unknown-linux-gnu` |
| Linux arm64 | `aarch64-unknown-linux-gnu` |

Linuxはglibc 2.28以上の環境が対象です。

以下はmacOS Apple Silicon向けの例です。別の環境では `target` を変更してください。

```sh
version=0.1.0
target=aarch64-apple-darwin
archive="jumeaux-${version}-${target}.tar.gz"
base="https://github.com/tadashi-aikawa/jumeaux-rs/releases/download/v${version}"
curl -fLO "${base}/${archive}"
curl -fLO "${base}/${archive}.sha256"
shasum -a 256 -c "${archive}.sha256"
```

チェックサムが `OK` になったことを確認してから展開します。
Linuxでは `sha256sum -c "${archive}.sha256"` でも検証できます。

```sh
tar -xzf "$archive"
mkdir -p "$HOME/.local/bin"
install -m 755 "jumeaux-${version}-${target}/jumeaux" "$HOME/.local/bin/jumeaux"
export PATH="$HOME/.local/bin:$PATH"
jumeaux --version
```

`jumeaux 0.1.0` と表示されれば導入完了です。
シェルの設定にも `$HOME/.local/bin` をPATHへ追加してください。

## 使い方

作業ディレクトリでテンプレートを生成します。

```sh
mkdir jumeaux-example
cd jumeaux-example
jumeaux init simple
jumeaux server
```

別のターミナルで同じディレクトリへ移動し、比較を実行します。

```sh
jumeaux run requests
jumeaux viewer
```

`responses/latest` に最新のレポートが作成されます。
Viewerは `http://localhost:5500/` で開きます。

`config.yml` の `one.host` と `other.host` を比較したいAPIへ変更し、`requests` にリクエストを記載します。

カスタムアドオンとPython transformerの代替は準備中です。

詳細: [カスタムアドオン](https://tadashi-aikawa.github.io/jumeaux-rs/ja/addons/custom/)

## 開発

Rustのバージョンは `mise.toml` を参照してください。

```sh
mise install
cargo build --locked
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

Python版との比較ハーネスは、Python版のソースと既存の `.venv` を使います。

```sh
export JUMEAUX_PYTHON_ROOT="$HOME/git/github.com/tadashi-aikawa/jumeaux"
cargo build --locked
JUMEAUX_BIN="$PWD/target/debug/jumeaux" uv run --project compat/harness python compat/harness/run.py
```

ハーネス自体を検証する場合は `JUMEAUX_BIN="$PWD/compat/harness/python-jumeaux"` に差し替えます。

詳細: [比較ハーネスの前提条件と実行手順](compat/README.md)

### ドキュメント

uvでビルド環境を管理します。

```sh
uv sync --locked
uv run zensical serve
uv run zensical build --clean --strict
```

入力は `docs/site/`、出力は `site/` です。

詳細: [ドキュメントの開発と公開](docs/documentation.md)

## リリース

配布先はGitHub Releasesです。

1. `Cargo.toml` と `Cargo.lock` のバージョンを更新します。
2. テストとビルドを確認し、変更をmainへマージします。
3. mainのリリース対象コミットにタグを付け、pushします。

```sh
git tag v0.1.0
git push origin v0.1.0
```

`release.yaml` がタグとCargoのバージョンを照合し、既定featureで `cargo test --locked` を実行します。
テスト成功後に4ターゲットをビルドします。
すべて成功すると `tar.gz` と各アーカイブの `.sha256` をGitHub Releaseへ公開します。
タグが `v<Cargo.tomlのversion>` と一致しない場合は失敗します。

詳細: [リリースの構成と検証](docs/releasing.md)
