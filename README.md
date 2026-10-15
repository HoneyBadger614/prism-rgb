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

```sh
cargo run --bin prism-probe
```

This lists connected Corsair devices. It is read-only and never sends anything
to a device.

## Project layout

- `crates/prism-core` - the device layer (finding and talking to devices)

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).

Prism's device code is written from scratch. Projects such as ckb-next and
OpenLinkHub are used only as references for how devices communicate; no code
is copied from them.
