# skkeleton 仕様確認メモ

参照: [skkeleton.jax](https://github.com/vim-skk/skkeleton/blob/main/doc/skkeleton.jax) の KEYMAPPINGS / CONFIG。

- 既定の `input` マップは BS、Ctrl-G、Ctrl-H、Ctrl-Q、CR、Esc、NL。`henkan` は Ctrl-G、CR、NL、Space、X、x。指示書の `q` / `L` / `/` / `l` 等は SKK 慣例として追加した。
- `keepMode=false`、`showCandidatesCount=4`、`immediatelyCancel=true`、`immediatelyDictionaryRW=true`、`immediatelyOkuriConvert=true`、`eggLikeNewline=false`、`markerHenkan=▽`、`markerHenkanSelect=▼`、`selectCandidateKeys=asdfjkl` を確認。
- `immediatelyOkuriConvert` は送り仮名入力中に「っ」を直接入力した時点でタ行送りと判断する。ローマ字列の途中で促音を組み立てる通常入力には影響しない。
- 読み補完は送りなしのユーザー辞書見出しを対象にし、`Tab` / `Shift-Tab` で辞書順に巡回する。Google 日本語入力、skkserv、Deno KV は対象外。
- 入力方法 v2 のプロトコルでは `done` イベントに引数がない。`commit(serial)` は、その object で発行済みの `done` の総数と一致させる。
- 第一段階は候補ポップアップを作らず preedit に候補を表示する。単語登録は候補なし時に登録待ち preedit を表示し、読みと送り仮名に対する語を受け付ける。
