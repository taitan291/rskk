# Third-party notices

The rskk project itself is licensed under the MIT License; see [LICENSE](LICENSE).
The following third-party components are referenced by this repository or its
Nix package and have separate license terms.

## Rust crates

Rust dependencies are listed in [Cargo.lock](Cargo.lock). Their SPDX license
expressions and license files are maintained by each crate upstream. Most use
MIT, Apache-2.0, or a choice of those licenses. Pay particular attention to:

| Crate | License declared upstream |
| --- | --- |
| `encoding_rs` | Apache-2.0 OR MIT, and BSD-3-Clause |
| `unicode-ident` | MIT OR Apache-2.0, and Unicode-3.0 |
| `slotmap` | Zlib |

Other crates may declare additional alternatives. Consult the license metadata
and license files in each crate's source distribution when redistributing the
application or its dependencies.

## Runtime dictionary

The Nix package references `SKK-JISYO.L` from nixpkgs' `skkDictionaries.l`;
the dictionary is not copied into this repository. SKK-JISYO.L is covered by
GNU GPL version 2 or later. See the [SKK dictionary license notes](https://github.com/skk-dev/dict/blob/master/committers.md).

## Noto CJK Sans

The Nix package uses Noto CJK Sans for candidate display. The font is licensed
under the SIL Open Font License, version 1.1. Its license and notices are
available in the [upstream Noto CJK repository](https://github.com/notofonts/noto-cjk/blob/main/Sans/LICENSE).
