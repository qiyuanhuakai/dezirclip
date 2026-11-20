# Unicode character catalog

The emoji panel in both desktop renderers reads the same generated `frontends/dzc-surface/cmd/dzc-desktop/emoji-data.json`. The catalog ships in the app and search works offline.

- Emoji entries use the Unicode 17 dataset and shortcodes from [emojis 0.8.2](https://docs.rs/emojis/0.8.2/emojis/).
- Standalone symbols use Unicode general categories and names from [unicode-general-category](https://docs.rs/unicode-general-category/) and [unicode_names2](https://docs.rs/unicode_names2/).
- Simplified and Traditional Chinese search terms come from [CLDR JSON 48.2.0](https://github.com/unicode-org/cldr-json/tree/48.2.0/cldr-json/cldr-annotations-full/annotations). The Unicode data is distributed under [Unicode License V3](UNICODE-LICENSE.txt).

Download `annotations.json` for `zh` and `zh-Hant` from that CLDR release. From the repository root, pass the two files to the generator in that order.

```sh
cargo run --locked --manifest-path scripts/emoji-catalog/Cargo.toml -- \
  zh.json zh-Hant.json frontends/dzc-surface/cmd/dzc-desktop/emoji-data.json
```

The SHA-256 hashes are `8d2dfe9bf41894bd17c9c1a1906a31bf0ccc06204bf4b77c2ab2b0d8f79a5fdf` for `zh` and `59158786a4a0b0937a7f3a370cbe5bc79ae7f3a6155edd55914dceadf0f0a35f` for `zh-Hant`.
