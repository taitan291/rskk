# rskk
codexの無料版トークンを枯らすために作らせた SKK

Rust 製の SKK 日本語入力システムです。I/O 非依存の入力エンジン (`skk-core`)、SKK 辞書の読込とユーザー辞書 (`skk-dict`)、Wayland `input-method-v2` クライアント (`rskk-wayland`) で構成しています。

## 起動

NixOS では `nix run .` で起動でき、`nix build` で `result/bin/rskk-wayland` を作れます。Nix パッケージには候補表示用の Noto CJK Sans を設定しています。`nix develop` は fenix の stable Rust、rust-analyzer、辞書、フォーマッタ、pre-commit フックを用意します。開発中の入力確認には `nix develop` の `RSKK_SYSTEM_DICT` を使えます。

### Nix で導入する

GitHub から直接実行・導入できます。

```sh
nix run github:taitan291/rskk
nix profile install github:taitan291/rskk
```

別の flake から使う場合は `inputs.rskk.url = "github:taitan291/rskk";` を追加し、`packages.${system}.default` を参照するか、`overlays.default` を適用して `pkgs.rskk-wayland` を使います。

`rskk-wayland` を Wayland セッションで起動してください。起動時に `zwp_input_method_manager_v2`、`wl_seat`、`zwp_virtual_keyboard_v1` が必要です。入力モードは **Ctrl-J** または **Super-Space** で切り替えます。

起動中の `rskk-wayland` には CLI からも切り替えを指示できます。`rskk-wayland direct` で直接入力モード、`rskk-wayland normal` で SKK 入力に戻ります。`$XDG_RUNTIME_DIR/rskk.sock` (`RSKK_SOCKET` で変更可) の Unix ソケット経由です。nvim の `InsertEnter` / `InsertLeave` や compositor のキーバインドから呼べます。直接入力モードでは Ctrl-J や Esc を含む全キーを素通しします。

ひらがな入力中に `K` で変換開始、`Space` で変換し、続けて `Space` / `x` / `asdfjkl` で候補を選びます。候補が多い場合は `zwp_input_popup_surface_v2` に候補一覧を表示します。送りなしモードで `Tab` / `Shift-Tab` を押すと、ユーザー辞書の見出し語補完を順方向 / 逆方向に選べます。`Enter` で確定します。`q` はひらがな / カタカナ、`Ctrl-Q` は半角カタカナ、`L` は全角英数、`/` は abbrev、`Ctrl-G` は取消です。

入力 method v2 を提供する compositor と `text-input-v3` を使う Wayland アプリが必要です。GTK / Qt の Wayland 版での動作を想定しています。XWayland アプリや、text-input を使わないアプリでは入力できないことがあります。Hyprland 上での実機試験はこの環境では行っていません。Wayland では seat ごとに `input-method-v2` を使う IME は一つだけ登録できます。`fcitx5` や `ibus` が起動中は `rskk-wayland` を同時に起動できないため、既存 IME を終了してから起動してください。Fcitx5 の場合は `fcitx5-remote -e` で終了できます。

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
key_repeat_rate_limit = 15 # 長押し時の最大反復回数/秒。0 で無効
key_repeat_delay_min = 500 # 長押しから反復開始までの最低待ち時間 (ms)
```

`RSKK_CONFIG` は設定ファイルを、`RSKK_SYSTEM_DICT` は `:` 区切りのシステム辞書を、`RSKK_USER_DICT` はユーザー辞書を指定します。ユーザー辞書の既定値は `$XDG_DATA_HOME/rskk/user-dict` (既定 `~/.local/share/rskk/user-dict`) です。システム辞書は設定順に読み込み、ユーザー辞書を先に検索します。ユーザー辞書は UTF-8 固定で、学習内容をバックグラウンドでアトミック保存します。

### 独自かなテーブル

標準ローマ字テーブルは `skk-core` に同梱した TOML です。独自テーブルは TOML の `[kana]` または `[tables.<名前>.kana]` セクションとして作り、設定に `global_kana_table_files = ["/path/to/table.toml"]` と `kana_table = "rom"` を指定できます。ファイルのキーはローマ字、値は出力する文字列です。同じキーは後から読んだテーブルで上書きされます。

## 辞書ライセンス

このリポジトリは SKK 辞書を同梱しません。nixpkgs の `skkDictionaries.l` を実行時に参照します。SKK-JISYO.L は GPL で配布されています。ユーザーが選択した外部辞書のライセンス条件に従ってください。

## ライセンス

rskk のライセンスは [MIT License](LICENSE) です。依存クレート、SKK 辞書、Noto CJK Sans フォントのライセンス情報は [第三者ライセンス表記](THIRD_PARTY_NOTICES.md) を参照してください。

## 現在の範囲と制約

- 候補は preedit にインライン表示し、多数候補の選択時は `zwp_input_popup_surface_v2` と共有メモリで描画します。ポップアップの実コンポジタ上での動作は未確認です。
- 候補なし時の単語登録は、入力した語をユーザー辞書へ登録する単段の基本フローです。登録中の再帰変換は未実装です。
- `Ctrl-G` は入力・変換・単語登録を取り消します。直前の確定を取り消す `kakuteiUndo` は未実装です。
- 数値変換は `#0`、`#1`、`#2`、`#3`、`#8` のみです。
- キー素通しは compositor の virtual-keyboard protocol に依存します。
- 読み補完は送りなしのユーザー辞書見出しのみが対象で、候補は見出し語の辞書順です。システム辞書、圧縮辞書、skkserv は対象外です。
- EUC-JP / UTF-8 の辞書を読み込みます。

skkeleton の確認事項と差異は [docs/skkeleton-notes.md](docs/skkeleton-notes.md)、設計判断は [docs/decisions.md](docs/decisions.md) に記録しています。
