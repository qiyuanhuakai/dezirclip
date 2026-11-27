# Unicode character catalog

The catalog lives at [assets/catalogs/emoji.json](../../assets/catalogs/emoji.json). React loads it directly. Go embeds a generated copy in its own package so it can build independently. Both renderers search offline.

- Emoji entries use the Unicode 17 dataset and shortcodes from [emojis 0.8.2](https://docs.rs/emojis/0.8.2/emojis/).
- Standalone symbols use Unicode general categories and names from [unicode-general-category](https://docs.rs/unicode-general-category/) and [unicode_names2](https://docs.rs/unicode_names2/).
- English, Simplified Chinese and Traditional Chinese search terms come from the [base annotations](https://github.com/unicode-org/cldr-json/tree/48.2.0/cldr-json/cldr-annotations-full/annotations) and [derived annotations](https://github.com/unicode-org/cldr-json/tree/48.2.0/cldr-json/cldr-annotations-derived-full/annotationsDerived) in CLDR JSON 48.2.0. The Unicode data is distributed under [Unicode License V3](UNICODE-LICENSE.txt).

Download the base and derived annotation files for `zh`, `zh-Hant` and `en` from that release. The generator reads `aliases.json` for common names and adds Chinese search terms for flags and arrow families.

```sh
cargo run --locked --manifest-path scripts/emoji-catalog/Cargo.toml -- \
  zh-base.json zh-Hant-base.json en-base.json \\
  zh-derived.json zh-Hant-derived.json en-derived.json \\
  assets/catalogs/emoji.json
node scripts/emoji-catalog/sync-embed.mjs
```

| File | SHA-256 |
| --- | --- |
| `zh-base.json` | `8d2dfe9bf41894bd17c9c1a1906a31bf0ccc06204bf4b77c2ab2b0d8f79a5fdf` |
| `zh-Hant-base.json` | `59158786a4a0b0937a7f3a370cbe5bc79ae7f3a6155edd55914dceadf0f0a35f` |
| `en-base.json` | `f22083cb86dffb63a643d5bacb5d9899f82d2fa5d388ad4f3aed72184acef505` |
| `zh-derived.json` | `1ff96249af6fc69067f2dfabc26b450786a93d8048f2764ecfad53fd64953f92` |
| `zh-Hant-derived.json` | `d7c618f1ac87131cf60e6e736899145e5e1cc152aa3a7015c07b6ec09ceac28e` |
| `en-derived.json` | `15c9457afa703a32dcc78d54626418a91e820fe0ddd67b7d491c4c4d9f5846ab` |

Both renderers rank glyph and whole-name matches before word fragments. If there are no direct results, one English word or Chinese phrase of four or more characters can differ by one insertion, deletion, replacement or adjacent swap.

CI and MyGo desktop packaging verify that the copy matches the source by SHA-256.

Regenerate the shared flag atlas and preview fonts with
`python scripts/emoji-catalog/build-previews.py` (requires Pillow and fontTools).
The artwork credits and font notices are in
[character-assets/NOTICE.md](../../frontends/dzc-surface/cmd/dzc-desktop/character-assets/NOTICE.md).
