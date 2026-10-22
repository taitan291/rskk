# rskk

Rust 製の SKK 日本語入力システムです。I/O 非依存の入力エンジン (`skk-core`)、SKK 辞書の読込とユーザー辞書 (`skk-dict`)、Wayland `input-method-v2` クライアント (`rskk-wayland`) で構成しています。

## 起動

NixOS では `nix build` で `result/bin/rskk-wayland` を作れます。Nix パッケージには候補表示用の Noto CJK Sans を設定しています。`nix develop` は fenix の stable Rust、rust-analyzer、辞書、フォーマッタ、pre-commit フックを用意します。開発中の入力確認には `nix develop` の `RSKK_SYSTEM_DICT` を使えます。

`rskk-wayland` を Wayland セッションで起動してください。起動時に `zwp_input_method_manager_v2`、`wl_seat`、`zwp_virtual_keyboard_v1` が必要です。入力モードは **Ctrl-J** または **Super-Space** で切り替えます。ひらがな入力中に `K` で変換開始、`Space` で変換し、続けて `Space` / `x` / `asdfjkl` で候補を選びます。候補が多い場合は `zwp_input_popup_surface_v2` に候補一覧を表示します。`Enter` で確定します。`q` はひらがな / カタカナ、`Ctrl-Q` は半角カタカナ、`L` は全角英数、`/` は abbrev、`Ctrl-G` は取消です。

入力 method v2 を提供する compositor と `text-input-v3` を使う Wayland アプリが必要です。GTK / Qt の Wayland 版での動作を想定しています。XWayland アプリや、text-input を使わないアプリでは入力できないことがあります。Hyprland 上での実機試験はこの環境では行っていません。別の IME が同じ seat を使う場合も競合することがあります。

## 設定

設定は `$XDG_CONFIG_HOME/rskk/config.toml` (既定 `~/.config/rskk/config.toml`) に置きます。

```toml
system_dictionaries = ["/usr/share/skk/SKK-JISYO.L"]
user_dictionary = "~/.local/share/rskk/user-dict"

keep_mode = false
show_candidates_count = 4
select_candidate_keys = "asdfjkl"
immediately_cancel = true
immediately_okuri_convert = true
immediately_dictionary_rw = true
register_convert_result = false
egg_like_newline = false
marker_henkan = "▽"
marker_henkan_select = "▼"
kana_table = "rom"
accept_illegal_result = false
```

`RSKK_CONFIG` は設定ファイルを、`RSKK_SYSTEM_DICT` は `:` 区切りのシステム辞書を、`RSKK_USER_DICT` はユーザー辞書を指定します。ユーザー辞書の既定値は `$XDG_DATA_HOME/rskk/user-dict` (既定 `~/.local/share/rskk/user-dict`) です。システム辞書は設定順に読み込み、ユーザー辞書を先に検索します。ユーザー辞書は UTF-8 固定で、学習内容をバックグラウンドでアトミック保存します。

### 独自かなテーブル

標準ローマ字テーブルは `skk-core` に同梱した TOML です。独自テーブルは TOML の `[kana]` または `[tables.<名前>.kana]` セクションとして作り、設定に `global_kana_table_files = ["/path/to/table.toml"]` と `kana_table = "rom"` を指定できます。ファイルのキーはローマ字、値は出力する文字列です。同じキーは後から読んだテーブルで上書きされます。

## 辞書ライセンス

このリポジトリは SKK 辞書を同梱しません。nixpkgs の `skkDictionaries.l` を実行時に参照します。SKK-JISYO.L は GPL で配布されています。ユーザーが選択した外部辞書のライセンス条件に従ってください。

## 現在の範囲と制約

- 候補は preedit にインライン表示し、多数候補の選択時は `zwp_input_popup_surface_v2` と共有メモリで描画します。ポップアップの実コンポジタ上での動作は未確認です。
- 候補なし時の単語登録は、入力した語をユーザー辞書へ登録する単段の基本フローです。登録中の再帰変換は未実装です。
- `Ctrl-G` は入力・変換・単語登録を取り消します。直前の確定を取り消す `kakuteiUndo` は未実装です。
- 数値変換は `#0`、`#1`、`#2`、`#3`、`#8` のみです。
- キー素通しは compositor の virtual-keyboard protocol に依存します。
- EUC-JP / UTF-8 の辞書を読み込みます。圧縮辞書、skkserv、補完は対象外です。

skkeleton の確認事項と差異は [docs/skkeleton-notes.md](docs/skkeleton-notes.md)、設計判断は [docs/decisions.md](docs/decisions.md) に記録しています。
