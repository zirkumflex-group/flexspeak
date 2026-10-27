# Lucide status icons

`lucide-status.ttf` contains fourteen glyphs from the official `lucide-static` 1.62.0 package. It is a 7,504-byte subset, rendered through Iced's existing text renderer. The original ISC/MIT notices are preserved in `LICENSE` and copied into app bundles and release dependency notices.

Source: https://registry.npmjs.org/lucide-static/-/lucide-static-1.62.0.tgz

Package integrity (SHA-512, base64): `WBZA/m2PvkFpMGHDtsBck5P2A9G4ehLlYu4GwwICcmEm1BvYVgj00lCDxKRWLlbMlOibLaiwdQ88G5pHkSWgwA==`

The selected icons are `cpu`, `memory-stick`, `mic`, `mic-off`, `headphones`, `headphone-off`, `volume-2`, `volume-x`, `server`, `hash`, `settings`, `chart-line`, `unplug`, and `monitor-play`. Their original codepoints are mapped in `src/icons.rs`. Recreate the subset after extracting the package:

```sh
uv run --no-project --with fonttools==4.67.0 pyftsubset package/font/lucide.ttf \
  --output-file=assets/lucide/lucide-status.ttf \
  --unicodes=U+E0A9,U+E445,U+E118,U+E119,U+E0F1,U+E629,U+E1AB,U+E1AC,U+E153,U+E0EF,U+E154,U+E2A5,U+E45D,U+E485 \
  --no-hinting
```

FontTools is only used when updating this checked-in asset; normal builds need no font tooling or icon downloads. Icons inherit their text color. Settings, Statistics and sidebar disconnect/mute/deafen controls use centered icons with descriptive hover tooltips. Voice and Streaming pair an icon with a label when space permits and use icon-only navigation in narrow windows.
