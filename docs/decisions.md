# 設計判断

- 2026-10-22: 空のリポジトリから指示書の3 crate 構成を作り、Rust stable 2024 edition と wayland-rs 0.31 系を採用。
- 2026-10-22: `purgeCandidate` のシステム辞書候補抑止は非標準形式を永続化せず、ユーザー辞書から削除した候補を実行中だけ抑止する。
- 2026-10-22: wayland `done` ごとに連番を増やし、その値を input-method-v2 `commit` の serial に使う。
- 2026-10-22: 開発シェルの rust-analyzer は fenix の GitHub ソース取得を避け、nixpkgs パッケージを使う。
- 2026-10-22: fenix の cargo-clippy が rustup shim を要求する環境では、同梱 clippy-driver を Cargo wrapper として使う。
- 2026-10-22: Nix の pre-commit derivation は Cargo registry にアクセスできないため無効化し、同じ tests/lints は `checks.quality` で実行する。
- 2026-10-22: xkbcommon は実行時共有ライブラリなので、Nix package に RPATH を付け、開発シェルにもライブラリパスを設定する。
- 2026-10-22: 候補一覧は cosmic-text でラスタライズし、input-popup-surface-v2 の wl_shm バッファとして表示する。
- 2026-10-22: 候補ポップアップ用に Noto CJK Sans を Nix パッケージと開発シェルへ設定し、隔離環境でも日本語フォント描画を検証する。
- 2026-10-22: NixOS x86_64-linux 上で SKK-JISYO.L 全読込は 142 ms、最大 RSS は 75,264 KiB (GNU time -v、Wayland 接続前) と計測。
