# Lucide icons

The interface draws icons from the official [Lucide](https://lucide.dev) set (`lucide-static` 1.62.0) as vector paths. Their SVG bodies live in `src/ui/icons.rs`; Rune rasterises them into its glyph atlas at the size and scale they are drawn, so no font or image asset is shipped. The original ISC/MIT notices are preserved in `LICENSE` and copied into app bundles and release dependency notices.

Source: https://registry.npmjs.org/lucide-static/-/lucide-static-1.62.0.tgz

To add an icon, copy its `<svg>` children from the package's `icons/<name>.svg` into `src/ui/icons.rs` with `fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"` on each element (Rune reads attributes per element, not from the outer `<svg>`). Icons take the colour they are drawn with.
