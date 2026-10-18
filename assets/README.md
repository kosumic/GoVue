# App icon

`icon.png` is the 1024 × 1024 black GoVue mark with an alpha-transparent
background, based on the supplied artwork. The play triangle is black and
all enclosed negative space is transparent.

Like Whip, the native icons keep the artwork separate from the platform
background. Android uses `adaptive-icon.png`, a centered 68% reduction of
`icon.png` on a transparent 1024 × 1024 canvas, for both regular and themed
adaptive icons. iOS uses `AppIcon.icon`, with the same transparent artwork
at 86% scale and no added shadow, specular, or translucency effects. Both
native icon containers have a white backing, matching Whip's configuration.
Expo prebuild generates the native resources from `app.json`; a native
rebuild is required for installed apps to pick up the icon.

The built-in imagegen tool edited the supplied artwork with this prompt:

> Use case: background-extraction. Edit target: supplied GoVue app icon.
> Make the entire red abstract human-shaped mark and its play triangle
> solid black (#000000). Remove the warm cream background completely,
> including every enclosed negative space, and replace it with real alpha
> transparency. Preserve the original artwork's exact silhouette, smooth
> antialiased edges, rounded line ends, internal play triangle, position,
> proportions, and 1024 by 1024 square canvas. Change only the color and
> background. No redesign, shadows, border, added objects, or checkerboard
> pixels.

The generated alpha was preserved when resizing the PNG assets with
Expo's installed `jimp-compact` renderer.
