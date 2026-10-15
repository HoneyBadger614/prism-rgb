# Prism

A Linux control center for Corsair devices: lighting, key bindings, and
saving profiles to onboard memory.

> Prism is an independent project. It is not made, endorsed, or supported by
> Corsair. Corsair and iCUE are trademarks of Corsair Gaming, Inc.

## Status

Early development. Prism can currently **find** these devices (it does not
control them yet):

| Device | USB ID |
|---|---|
| K65 RGB Mini | `1b1c:1baf` |
| Nightsword RGB | `1b1c:1b5c` |
| Lighting Node Pro | `1b1c:0c0b` |

## Try it

You need Rust, Node.js, `hidapi` and `webkit2gtk-4.1`. On Arch:

```sh
sudo pacman -S rust nodejs npm hidapi webkit2gtk-4.1
npm install
npm run dev
```

To add Prism (with its icon) to your app launcher and taskbar:

```sh
scripts/install-desktop.sh            # add
scripts/install-desktop.sh --remove   # remove
```

To list connected Corsair devices in the terminal instead:

```sh
cargo run --bin prism-probe
```

`prism-probe` is read-only and never sends anything to a device.

## Project layout

- `crates/prism-core` - the device layer (finding and talking to devices)
- `src-tauri` - the desktop app (window, and the bridge to `prism-core`)
- `ui` - the screens (HTML, CSS, JavaScript)

## Security

- Prism never needs to run as root.
- The app window has no file, shell, or network access. It can only call
  Prism's own commands.
- No fonts, scripts, or anything else load from the internet.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).

Prism's device code is written from scratch. Projects such as ckb-next and
OpenLinkHub are used only as references for how devices communicate; no code
is copied from them.
