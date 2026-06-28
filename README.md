# DezirClip

DezirClip is a local clipboard workspace for Windows and Linux, built with Tauri 2, Rust, React, and TypeScript.

It focuses on desktop-local clipboard history, quick paste, screenshots, OCR-assisted image search, text transforms, tags, backup import/export, and the `dzc` command-line tool.

## Highlights

- Windows and Linux support
- Local SQLite clipboard history
- Quick-paste overlay and multiple hotkeys
- Region screenshot capture
- OCR text storage and search for copied images
- Rich text, image, file, URL, code, and tag-aware clipboard entries
- Theme engine with six bundled themes
- `dzc` CLI for add/list/search/get/tag/import/export workflows
- Agent Skill under [`skills/dzc-cli`](./skills/dzc-cli/)

## Linux dependencies

Ubuntu/Debian:

```bash
sudo apt update
sudo apt install -y libgtk-3-dev libwebkit2gtk-4.0-dev libappindicator3-dev xdotool
```

Arch Linux:

```bash
sudo pacman -S gtk3 webkit2gtk libappindicator-gtk3 xdotool
```

Fedora:

```bash
sudo dnf install gtk3-devel webkit2gtk3-devel libappindicator-gtk3-devel xdotool
```

## Development

```bash
npm install
npm run tauri:dev
npm run tauri:build
```

Build artifacts are written under `src-tauri/target/release/bundle/`.

## CLI and Agent Skill

The CLI binary is named `dzc`.

```bash
dzc --help
```

The Agent Skill is available in [`skills/dzc-cli`](./skills/dzc-cli/). Copy or link that directory into your Agent skills directory.

## License and attribution

DezirClip is licensed under GPL-3.0.

This project is derived from [jimuzhe/tiez-clipboard](https://github.com/jimuzhe/tiez-clipboard), which is also GPL-3.0 licensed. Original copyright and license notices are retained where applicable.
