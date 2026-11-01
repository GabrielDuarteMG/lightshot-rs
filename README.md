# lightshot-rs

Screenshot tool for Linux, Windows, and macOS. It grabs every monitor, lets you drag a region, annotate it, then copy or save. The interaction follows Lightshot. The code here was written for this repository.

If every monitor reports origin 0,0, only the largest monitor is kept.

This is not Lightshot and it is not affiliated with Skillbrains. Lightshot is their product and their name. Nothing here uploads to prnt.sc. This repository contains no Lightshot binary, icons, artwork, or decompiled source.

The behavior was studied with [REA](https://github.com/morluto/rea), which is MIT. REA's source is not included. Running that tool does not make this program a copy of REA.

## License

The code and docs in this repository are MIT. See [LICENSE](LICENSE). That license covers this repository only.

Lightshot stays under Skillbrains' own terms. This license does not grant any right to their software or their name.

REA stays under the MIT license in [morluto/rea](https://github.com/morluto/rea). This repository does not redistribute REA, so REA's copyright notice is not shipped here.

Observed behavior is not copyrighted expression. The Rust in `src/` is the expression being licensed.

This is not legal advice.

## Requirements

- Linux, Windows, or macOS desktop session that [xcap](https://crates.io/crates/xcap) can capture
- A Rust toolchain that can build edition 2021
- GNOME with `gsettings`, only for `install-hotkey`. The hotkey installer is GNOME only.
- A TTF font for burning text annotations into the saved image, first readable of: Linux `/usr/share/fonts/TTF/DejaVuSans.ttf`, `/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf`; Windows `C:\Windows\Fonts\arial.ttf`, `C:\Windows\Fonts\segoeui.ttf`; macOS `/System/Library/Fonts/Supplemental/Arial.ttf`, `/Library/Fonts/Arial.ttf`. Without one the saved image omits text; the on-screen editor draws text either way.

Clipboard access goes through [arboard](https://crates.io/crates/arboard) with `image-data`, plus `wayland-data-control` on Linux. On Linux, after you copy, the process forks a child that keeps serving the image until another client replaces the clipboard. Windows and macOS keep the image in the clipboard after the process exits, so there is no fork.

## Build

```bash
cargo build --release
./target/release/lightshot
```

## Releases

Pushing a `v*` tag builds and tests on GitHub-hosted runners and attaches these assets to the GitHub Release:

- `lightshot-linux-x86_64.tar.gz` with `lightshot`
- `lightshot-windows-x86_64.zip` with `lightshot.exe`
- `lightshot-macos-aarch64.tar.gz` with `lightshot` (Apple Silicon)
- `lightshot-macos-x86_64.tar.gz` with `lightshot` (Intel)

CI compiles and runs the unit tests on each OS; it does not run the GUI. The macOS binaries are unsigned, so Gatekeeper blocks them until you allow them in System Settings or run `xattr -d com.apple.quarantine lightshot`.

## Commands

```text
lightshot
lightshot capture
lightshot install-hotkey
lightshot uninstall-hotkey
```

`lightshot` with no arguments is capture. `install-hotkey` adds a GNOME custom binding named Lightshot on `<Ctrl>Print` that runs `<absolute-path-of-this-binary> capture`. Move the binary and run `install-hotkey` again. `uninstall-hotkey` removes that binding. On Windows and macOS both commands exit with an error that prints this binary's absolute path; bind Ctrl+Print to `lightshot capture` yourself.

A second capture exits immediately if one is already holding `capture.lock` in the config directory.

## Use

Drag to select. A toolbar sits at the bottom of the screen: Select, Pen, Line, Arrow, Rect, Marker, Text, eight colors, Undo, Copy, Save, Close.

| Input | Action |
| --- | --- |
| Drag outside the selection | New region. Annotations on the old region are dropped. |
| Drag inside, Select tool | Move the region |
| Handles | Resize. Annotations are kept only when they still fit. |
| Right click, Esc, Close | Quit |
| Enter, Ctrl+C, Copy | Copy the region and quit when `auto_close` is true |
| Ctrl+S, Save | Save dialog. Starts in `last_saved_dir` when that path is an existing directory, otherwise, on Linux only, the `XDG_PICTURES_DIR` path from `~/.config/user-dirs.dirs` when that path is an existing directory, otherwise `~/Pictures` when that directory exists, otherwise the current working directory. |
| Ctrl+Z, Undo | Drop the last annotation |
| Text, click inside the region | Type. Esc while the field is focused cancels that text. Leaving the field commits it. Ctrl+S still saves. |

With `auto_copy` true, releasing a new selection copies it and shows a notification. That path does not quit by itself.

The save dialog suggests `Screenshot_<n>.png` or `Screenshot_<n>.jpg`. `save_index` remembers the next number and skips names that already exist. A "Screenshot saved" notification can open the folder.

Copy always sends raw RGBA to the clipboard. JPEG is only a file encoding, used when the chosen extension is `.jpg` or `.jpeg`, or when `copy_format` is `jpeg` and the path has no png/jpeg extension. Quality comes from `jpeg_quality`.

## Config

Linux: `$XDG_CONFIG_HOME/lightshot/config.toml`, or `~/.config/lightshot/config.toml` when `XDG_CONFIG_HOME` is unset. macOS: `~/Library/Application Support/lightshot/config.toml`. Windows: `%APPDATA%\lightshot\config.toml`, or `%USERPROFILE%\lightshot\config.toml` when `APPDATA` is unset.

```toml
copy_format = "png"
jpeg_quality = 80
auto_copy = false
auto_close = true
last_saved_dir = ""
save_index = 1
```

`copy_format` accepts `png` or `jpeg`. Any other value loads as `png` and the file on disk is left as-is. `jpeg_quality` is clamped to 1..=100. `save_index` below 1 loads as 1.

## What it does not do

- No upload, no prnt.sc link, no account
- No hotkey installer outside GNOME `gsettings`
