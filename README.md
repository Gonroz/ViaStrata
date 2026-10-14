# Via Strata

Draw roads to real-world scale over satellite imagery. Enter coordinates to
fetch imagery with the scale set automatically, or open your own image and
calibrate the scale by clicking two points a known distance apart.

The goal is a standalone tool for sketching street-design concepts over real
imagery — the kind of work that currently requires a stack of Adobe apps and
a GIS tool.

cargo run --release


On first launch the app fetches a default location (an intersection in
Buffalo, NY) and drops you into the Road tool with the scale already set.

## What it does

**Imagery**
- Fetch Web-Mercator tiles from Esri, Esri Clarity, or USGS, centered on a
  `lat, lon`, and stitch them into a square. Scale is derived from the zoom
  level and latitude — no calibration needed.
- Or open a local PNG/JPEG/WebP and calibrate it by clicking two points a
  known distance apart.
- The image is drawn with mipmapped linear filtering, so both the
  fit-to-window view and pixel-level zoom stay clean.

**Roads**
- Click to place control points. The road is rendered as a Catmull-Rom
  spline through those points, so it curves smoothly while still passing
  through every click.
- Each road has a real-world width (`lanes × lane_width`) and its own
  center-line style. Width is derived per frame from `px_per_m × zoom`, so
  a road keeps its physical size at any zoom.
- Lane markings are drawn to US convention: solid white edge lines, dashed
  white lane dividers, and a style-selectable center line.

**Node graph**
- Roads are built from *nodes* — shared points stored in a single graph.
  Two roads that reference the same node stay glued together: dragging the
  node moves both roads at once.
- Shared nodes get a joint fill at the vertex so two roads meeting at an
  angle look like a real intersection rather than two butt ends overlapping.
- Shared nodes are drawn with a ring around their handle so you can tell at
  a glance which points are welded.

## Controls

| Gesture | Result |
|---|---|
| Left-click | Place a control point (Road tool) / set a calibration point (Calibrate tool) |
| Left-click an existing node | Terminate the road there, or start a new road from it |
| Left-click a node handle | Grab and drag; every road at that node moves with it |
| Left-drag (Pan tool) | Pan |
| Middle-drag | Pan (always available) |
| Scroll wheel | Zoom around the cursor |
| W A S D | Pan (works in any tool) |
| Right-click (no draft) | On a node: delete it. On a segment: insert a node there |
| Right-click (draft in progress) | Finish the road |
| Double-click, Enter | Finish the road |
| Escape | Discard the current draft |
| Ctrl+Z | Undo the last point, then the last road |
| Delete / Backspace over a node | Delete that node |

Right-drag is *not* a pan gesture. Right-click has real actions (finish,
delete, insert), and mixing those with a pan produced accidental hits, so
panning is middle-drag or WASD only.

## Layout

`src/
├── main.rs entry point, window setup
├── app.rs App state, high-level operations, update loop
├── geo.rs Web-Mercator math, "lat, lon" parsing
├── imagery.rs tile sources, download, stitch, crop
├── spline.rs Catmull-Rom tessellation + polyline offset
├── model/
│ ├── mod.rs re-exports
│ ├── tool.rs Tool (Pan / Calibrate / Road)
│ ├── unit.rs Unit (Feet / Meters)
│ ├── road.rs Road, CenterStyle
│ └── graph.rs NodeId, NodeGraph
└── ui/
├── mod.rs
├── toolbar.rs top toolbar (three rows) + status bar
├── sidebar.rs right-side visual pickers
├── paint.rs low-level drawing helpers
└── canvas/
├── mod.rs orchestration + Xform
├── view.rs camera: fit, zoom, pan, WASD
├── input.rs mouse + keyboard actions
└── render.rs drawing the scene`

## Design notes

- **The node graph is the glue.** Roads store `Vec<NodeId>`, not
  `Vec<Pos2>`. Positions live in a single `NodeGraph`. That's what makes
  "drag a node and watch two roads move together" work without any
  synchronization logic.
- **Two-pass rendering.** All asphalt (with joint fills at shared nodes)
  is drawn before any markings. That way a joint fill never covers another
  road's lane lines, and a later road never erases an earlier one's
  asphalt at the shared vertex.
- **Catmull-Rom over Bézier.** Catmull-Rom is *interpolating*: the curve
  passes through every control point, so the user's clicks remain the
  literal truth of the road. Bézier would pull the road away from where
  they clicked, which is wrong for a design tool.
- **Right-click detection is custom.** egui's built-in click detection on
  a `click_and_drag` widget requires pixel-perfect press-and-release;
  hand jitter of a pixel or two gets classified as a drag. We record the
  press position and decide on release with an 8px tolerance.

## Imagery notes

- Tiles are Web Mercator, so `meters_per_pixel = 156543.034 * cos(lat) / 2^zoom`.
- USGS imagery is public domain but only reaches zoom 16 (~1.75 m/px).
- Esri World Imagery is sharper; check Esri's terms before anything beyond
  personal use.
- Esri and its Clarity layer top out at zoom 19. Higher zoom levels return
  upscaled tiles rather than new detail — the app permits z20 so you can
  experiment, but it's not sharper than z19.
- Keep the imagery attribution visible when sharing screenshots or exports.

## Known limitations

- Roads don't yet have per-lane types (parking, bike, turn lanes), so a
  4-lane road is four identical lanes. Crosswalks, stop bars, and curbs
  are also not modeled yet.
- Undo is a single-step pop, not a full history.
- Nothing is saved to disk yet — closing the app loses the scene.
