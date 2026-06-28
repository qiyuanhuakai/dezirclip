<div align="center">

<img src="docs/images/show.png" alt="DezirClip Logo" />

# DezirClip

A lightweight cross-platform clipboard manager focused on speed, practical daily workflows, and a polished desktop experience.

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)
[![Version](https://img.shields.io/badge/version-0.3.1-green.svg)](https://github.com/qiyuanhuakai/dezirclip/releases)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux-lightgrey.svg)](https://github.com/qiyuanhuakai/dezirclip/releases)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-4a90d9.svg)](https://tauri.app/)

[English](./README.md) | [简体中文](./README.zh-CN.md)

[Releases](https://github.com/qiyuanhuakai/dezirclip/releases) · [Issues](https://github.com/qiyuanhuakai/dezirclip/issues)

</div>

---

## Preview

<div align="center">
  <img src="docs/images/ui预览.png" alt="DezirClip UI Preview" width="860" />
</div>

## Overview

**DezirClip** is a cross-platform clipboard manager built with [Tauri 2](https://tauri.app/), available on **Windows** and **Linux**. It stays in the system tray, opens instantly with a global shortcut, and helps you manage clipboard history, rich text, tags, and everyday copy/paste workflow in one place.

## Highlights

| Highlight | Description |
| --- | --- |
| Fast | Open instantly with `Alt+V` and keep common actions close |
| Complete | Text, images, rich text, tags, and emoji |
| Practical | Designed as a daily background utility, not a demo app |
| Flexible | Themes, hotkeys, and persistence behavior are configurable |

## Features

### 1. Capture and Monitoring

- Native system clipboard event-driven listener, not polling
- Plain text capture
- Rich text (HTML) capture
- Automatic image capture with `.png` external storage
- File and folder path tracking
- Hash-based deduplication
- Code snippet type detection

### 2. Storage Management

- Configurable history size limit
- Pinned item protection from cleanup
- Tagged item protection from cleanup
- Periodic automatic cleanup of old records
- Day-based history grouping
- Usage count statistics

### 3. Browsing and Search

- Full-text content search
- Source application name search
- Tag-based filtering
- Compact / detailed preview modes
- Pinned items shown first
- Paginated history loading

### 4. Organization and Actions

- Multi-color custom tag system
- Global tag rename and management
- Pin / unpin records
- Manual drag sorting for pinned items
- Delete single or multiple records
- One-click clear for non-protected items
- JSON export / import

### 5. Interaction Flow and External Editing

- Global shortcut to summon the interface
- Open content in an external editor or handler
- Sequential paste mode
- Click / Enter to paste
- Optional auto-pin after paste
- Optional auto-delete after paste

### 6. Security and Privacy

- End-to-end database encryption
- Automatic encryption for sensitive-tagged records
- Regex-based masking for ID cards, phone numbers, emails, and other private data

### 7. Networking and Multi-Device

- Conflict handling across multiple devices

### 8. System Personalization

- Mica / Acrylic background effects
- Dark / regular mode with system follow
- Window opacity control
- Edge docking and always-on-top behavior
- Popup near mouse position
- Tray icon visibility control
- Auto-start management
- Sound effects toggle

## Requirements

| Platform | Requirement |
| --- | --- |
| Windows | Windows 10/11 (x64); Windows 10 requires [Microsoft Edge WebView2](https://developer.microsoft.com/en-us/microsoft-edge/webview2/) |
| Linux | Linux 10.15 Catalina or later (Apple Silicon / Intel) |

## Quick Start

Download the latest installer from [Releases](https://github.com/qiyuanhuakai/dezirclip/releases).

| Platform | Package |
| --- | --- |
| Windows | `.exe` installer / `.zip` portable build |
| Linux | `.dmg` disk image |

## Support and Community

If DezirClip is useful to you, sponsorship helps keep the project moving.

- Please leave a name or nickname with your donation so it can be added to the [sponsor list](https://github.com/qiyuanhuakai/dezirclip/zh/sponsors.html)
- You can also join the QQ group by scanning the QR code below

<div align="center">
  <table>
    <tr>
      <td align="center">
        <p><strong>WeChat</strong></p>
        <img src="docs/images/wx.jpeg" alt="WeChat donation QR" width="220" />
      </td>
      <td align="center">
        <p><strong>Alipay</strong></p>
        <img src="docs/images/zfb.jpeg" alt="Alipay donation QR" width="220" />
      </td>
    </tr>
  </table>
  <p><strong>QQ Group</strong></p>
  <img src="docs/images/qq.jpeg" alt="QQ group QR" width="220" />
</div>

## Developer

### Agent Skill

This project provides an Agent Skill for dzc-cli at [skills/dzc-cli/](./skills/dzc-cli/). Install with: `bash skills/dzc-cli/install.sh` (Linux/Linux) or `powershell -ExecutionPolicy Bypass -File skills/dzc-cli/install.ps1` (Windows).

---

<div align="center">

If DezirClip helps your workflow, a Star is appreciated.

</div>
