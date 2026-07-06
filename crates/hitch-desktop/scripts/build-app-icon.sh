#!/usr/bin/env bash
# Build the macOS-style app-icon sources: the Hitch knot (from hitch.svg)
# centered on a rounded-square "squircle" tile with the standard macOS ~10%
# margin, in two appearance variants:
#   - light: cream tile, red knot        -> app-icon.png       (fed to `tauri icon`)
#   - dark:  graphite tile, cream knot   -> app-icon-dark.png  (+ 512px icon-dark.png
#                                            embedded for the runtime Dock swap)
#
# Requires ImageMagick (`magick`).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../../.." && pwd)" # repo root
SVG="$ROOT/hitch.svg"
CRATE="$ROOT/crates/hitch-desktop"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# Icon geometry on a 1024 canvas. A slightly smaller margin + a larger knot make
# the mark fill more of the tile, matching how other Dock icons use their space.
CANVAS=1024
MARGIN=76                                # transparent margin around the tile
TILE=$((CANVAS - 2 * MARGIN))            # tile side (872)
RADIUS=$(((TILE * 2237) / 10000))        # macOS squircle corner radius (~22.37%)
KNOTH=$(((TILE * 76) / 100))             # knot height as a share of the tile

# Knot, rendered high then trimmed to its bounding box for precise sizing.
magick -background none "$SVG" -resize 1024x1024 "$work/knot.png"
magick "$work/knot.png" -trim +repage "$work/knot-trim.png"

# Rounded-square mask. Uses a transparent (xc:none) background so the mask has a
# real alpha channel — CopyOpacity copies that alpha, actually rounding the tile.
# (xc:black has no alpha, so CopyOpacity would leave the corners square.)
magick -size ${TILE}x${TILE} xc:none -fill white \
  -draw "roundrectangle 0,0,$((TILE - 1)),$((TILE - 1)),${RADIUS},${RADIUS}" "$work/mask.png"

# build_tile <gradient> <knot-fill|"keep"> <output>
build_tile() {
  local grad="$1" kc="$2" out="$3"
  magick -size ${TILE}x${TILE} gradient:"$grad" "$work/grad.png"
  magick "$work/grad.png" "$work/mask.png" -compose CopyOpacity -composite "$work/tile.png"
  # Bake subtle macOS-style lighting into the tile — a soft top highlight and a
  # faint bottom shade (macOS no longer auto-glosses Dock icons; the depth on
  # Apple's own icons is baked into their artwork). Each is clipped to the tile
  # via DstIn so the gradient's alpha is preserved (not flattened).
  magick -size ${TILE}x${TILE} gradient:'rgba(255,255,255,0.20)-rgba(255,255,255,0)' "$work/sheen.png"
  magick "$work/sheen.png" "$work/mask.png" -compose DstIn -composite "$work/sheen.png"
  magick -size ${TILE}x${TILE} gradient:'rgba(0,0,0,0)-rgba(0,0,0,0.10)' "$work/shade.png"
  magick "$work/shade.png" "$work/mask.png" -compose DstIn -composite "$work/shade.png"
  magick "$work/tile.png" "$work/shade.png" -compose over -composite "$work/tile.png"
  magick "$work/tile.png" "$work/sheen.png" -compose over -composite "$work/tile.png"
  if [ "$kc" = "keep" ]; then
    cp "$work/knot-trim.png" "$work/kn.png"
  else
    magick "$work/knot-trim.png" -channel RGB -fill "$kc" -colorize 100 +channel "$work/kn.png"
  fi
  magick "$work/kn.png" -resize x${KNOTH} "$work/knot-fit.png"
  magick -size ${CANVAS}x${CANVAS} xc:none "$work/tile.png" -geometry +${MARGIN}+${MARGIN} -composite "$work/base.png"
  magick "$work/base.png" "$work/knot-fit.png" -gravity center -composite "$out"
}

build_tile '#f6efe2-#e6d8c0' keep      "$CRATE/app-icon.png"
build_tile '#303136-#171719' '#f1e7d4' "$CRATE/app-icon-dark.png"

# 512px dark variant embedded by the app for the runtime dark-mode Dock swap.
magick "$CRATE/app-icon-dark.png" -resize 512x512 "$CRATE/src-tauri/icons/icon-dark.png"

echo "Wrote app-icon.png, app-icon-dark.png, src-tauri/icons/icon-dark.png"
