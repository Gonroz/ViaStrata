# Via Strata

Draw roads to real-world scale over satellite imagery. Enter coordinates to
fetch imagery with the scale set automatically, or open your own image and
calibrate the scale by clicking two points a known distance apart.

```
cargo run --release
```

## Layout

```
src/
├── main.rs        entry point, window setup
├── app.rs         App state, image loading, fetch lifecycle, update loop
├── model.rs       Tool, Unit, Road
├── geo.rs         Web-Mercator math, "lat, lon" parsing
├── imagery.rs     tile sources, download, stitch, crop
└── ui/
    ├── mod.rs
    ├── toolbar.rs three toolbar rows + status bar
    ├── canvas.rs  pan/zoom, clicks, drawing layers
    └── paint.rs   road + attribution painters
```

## Notes

- Tiles are Web Mercator, so scale = 156543.034 * cos(lat) / 2^zoom meters per pixel.
- USGS imagery is public domain but only reaches zoom 16 (~1.75 m/px near Buffalo).
- Esri World Imagery is sharper; check Esri's terms before anything beyond personal use.
- Keep the imagery attribution visible when sharing screenshots or exports.
