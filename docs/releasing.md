# リリースの構成と検証

## 成果物

タグ `v<Cargo.tomlのversion>` のpushで `.github/workflows/release.yaml` を起動する。
versionジョブでCargoのパッケージ情報を取得し、タグとの一致を検証する。
バージョンが一致しない場合、ビルドとRelease作成は実行しない。

既定featureで `cargo test --locked` を実行し、成功後に各ターゲットをビルドする。
テスト失敗時はビルドとRelease作成を実行しない。
リリース用ジョブではRustのビルドキャッシュを使わない。
全ワークフローのActionはコミットSHAに固定し、コメントにバージョンを記す。

| ターゲット | ランナー | ビルド |
| --- | --- | --- |
| aarch64-apple-darwin | macos-15 | cargo build |
| x86_64-apple-darwin | macos-15-intel | cargo build |
| x86_64-unknown-linux-gnu | ubuntu-24.04 | cargo zigbuild |
| aarch64-unknown-linux-gnu | ubuntu-24.04 | cargo zigbuild |

各ターゲットについて以下をGitHub Releaseへ添付する。

```text
jumeaux-0.1.0-<target>.tar.gz
jumeaux-0.1.0-<target>.tar.gz.sha256
```

アーカイブのルートは `jumeaux-0.1.0-<target>/`。
バイナリ `jumeaux`、`LICENSE`、`README.md` を含める。
チェックサムにはアーカイブ名だけを記録する。
ダウンロードしたディレクトリでそのまま検証できる。

全4ターゲットのビルドとチェックサム検証が成功してからReleaseを作る。
プレリリースのバージョンはGitHub Releaseでもprereleaseにする。

## Linuxのクロスビルド

cargo-zigbuild 0.24.1とZig 0.15.2を固定する。
Linuxの両アーキテクチャを `--target <target>.2.28` でビルドする。

- 選定理由: Zigがクロスリンカーとglibcを提供する。arm64用のDockerイメージを別途管理せずにビルドできる。
- glibc: x86_64とarm64の両方で2.28を指定し、ランナーのglibcへ直接依存するビルドを避ける。
- TLS: rustlsを使うため、OpenSSLのクロスビルドは不要。

詳細: [cargo-zigbuild公式README](https://github.com/rust-cross/cargo-zigbuild#readme)

```sh
uv tool install cargo-zigbuild==0.24.1 --with ziglang==0.15.2
export PATH="$(uv tool dir --bin):$PATH"
export CARGO_ZIGBUILD_PYTHON_PATH="$(uv tool dir)/cargo-zigbuild/bin/python"
rustup target add aarch64-unknown-linux-gnu
cargo zigbuild --locked --release --target aarch64-unknown-linux-gnu.2.28 --bin jumeaux
```

## ローカル検証

GitHub Actionsと同じmacOS向けビルドコマンドを使う。

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo build --locked --release --target aarch64-apple-darwin --bin jumeaux
cargo build --locked --release --target x86_64-apple-darwin --bin jumeaux
```

Linuxのバイナリはglibc 2.28を下限としてビルドする。
初回リリース時は各OSでの起動と、ダウンロードからチェックサム検証までを確認する。
