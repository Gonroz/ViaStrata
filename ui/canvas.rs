//! The central canvas: pan/zoom, clicks, drawing, and control-point editing.

use eframe::egui::{self, pos2, Color32, PointerButton, Pos2, Rect, Stroke, Vec2};

use super::paint::{paint_attribution, paint_road};
use crate::app::App;
use crate::model::{Tool, VertexRef};
use crate::spline;

/// Screen-space radius within which a press is treated as grabbing an
/// existing control point rather than placing a new one.
///
/// This is intentionally small (8 px) so that the handles do not swallow
/// clicks meant for the road itself.
const VERTEX_HIT_RADIUS: f32 = 8.0;

/// Visual radius of a rendered control-point handle in screen pixels.
const HANDLE_RADIUS: f32 = 4.0;

pub fn show(app: &mut App, ctx: &egui::Context) {
    egui::CentralPanel::default().show(ctx, |ui| {
        let (resp, painter) =
            ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
        let rect = resp.rect;

        if app.tex.is_none() {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Enter coordinates above, or open an image",
                egui::FontId::proportional(20.0),
                Color32::GRAY,
            );
            return;
        }

        // -------- View: fit / zoom / pan ------------------------------

        // One-shot fit on first display of a new image.
        if app.fit_pending {
            let z = (rect.width() / app.img_size.x).min(rect.height() / app.img_size.y) * 0.95;
            app.zoom = z;
            app.offset = (rect.size() - app.img_size * z) / 2.0;
            app.fit_pending = false;
        }

        // Zoom around the cursor: compute the image point under the cursor
        // *before* changing zoom, then re-solve offset so that point stays
        // pinned under the cursor afterwards.
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                if let Some(mp) = resp.hover_pos() {
                    let old = app.zoom;
                    let new = (old * (scroll * 0.002).exp()).clamp(0.02, 60.0);
                    let img_pt = (mp - rect.min - app.offset) / old;
                    app.zoom = new;
                    app.offset = mp - rect.min - img_pt * new;
                }
            }
        }

        // Pan is available on middle/right drag regardless of tool, and on
        // primary drag when the Pan tool is active.
        let pan = resp.dragged_by(PointerButton::Middle)
            || resp.dragged_by(PointerButton::Secondary)
            || (app.tool == Tool::Pan && resp.dragged_by(PointerButton::Primary));
        if pan {
            app.offset += resp.drag_delta();
        }

        // Snapshot transforms by value so `app` stays mutable inside the
        // closures we're about to use.
        let zoom = app.zoom;
        let offset = app.offset;
        let to_s = move |p: Pos2| rect.min + offset + p.to_vec2() * zoom;
        let to_i = move |s: Pos2| ((s - rect.min - offset) / zoom).to_pos2();

        if app.tool != Tool::Pan && resp.hovered() {
            ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
        }

        // -------- Vertex editing (Road tool only) ---------------------
        //
        // Ordering matters here: we compute hover state *before* click
        // handling, so a quick press/release on a handle is correctly
        // classified as "grab" (via hover) rather than "place a new point".
        let hit_radius_img = VERTEX_HIT_RADIUS / zoom;
        app.hover_vertex = if app.tool == Tool::Road {
            resp.hover_pos()
                .map(to_i)
                .and_then(|p| find_vertex(app, p, hit_radius_img))
        } else {
            None
        };

        if app.tool == Tool::Road {
            // Begin drag: only if the press landed on a handle.
            if resp.drag_started_by(PointerButton::Primary) {
                if app.hover_vertex.is_some() {
                    app.dragging = app.hover_vertex;
                }
            }

            // Continue drag: apply this frame's delta, divided by zoom so the
            // motion tracks the cursor 1:1 in image space regardless of view.
            if let Some(v) = app.dragging {
                let delta = resp.drag_delta() / zoom;
                if delta != Vec2::ZERO {
                    shift_vertex(app, v, delta);
                }
            }

            // End drag: clear on release, whether or not we moved.
            if resp.drag_stopped_by(PointerButton::Primary) {
                app.dragging = None;
            }

            // Delete the hovered handle on Delete or Backspace. We gate on
            // `!wants_keyboard_input` so a text field elsewhere keeps its
            // keystrokes.
            if app.hover_vertex.is_some()
                && !ctx.wants_keyboard_input()
                && ctx.input(|i| {
                    i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)
                })
            {
                if let Some(v) = app.hover_vertex {
                    delete_vertex(app, v);
                    app.hover_vertex = None;
                    app.dragging = None;
                }
            }
        }

        // -------- Clicks ----------------------------------------------
        //
        // A press that landed on a handle was consumed by the drag logic
        // above. We still see a `clicked` event if the user pressed and
        // released without moving — that's exactly the case we want to
        // suppress, so it doesn't drop a duplicate point on top of the
        // existing one.
        if resp.double_clicked_by(PointerButton::Primary) && app.tool == Tool::Road {
            app.finish_road();
        } else if resp.clicked_by(PointerButton::Primary) {
            if let Some(pos) = resp.interact_pointer_pos() {
                let ip = to_i(pos);
                let on_handle = app.hover_vertex.is_some();
                match app.tool {
                    Tool::Calibrate => {
                        // A third click restarts the measurement. Two clicks
                        // then Apply is the normal path.
                        if app.calib_pts.len() >= 2 {
                            app.calib_pts.clear();
                        }
                        app.calib_pts.push(ip);
                    }
                    Tool::Road => {
                        if on_handle {
                            // Grabbed an existing point; nothing to add.
                        } else if app.px_per_m.is_some() {
                            app.current.push(ip);
                        } else {
                            app.status =
                                "Set the scale first (fetch satellite or calibrate).".into();
                        }
                    }
                    Tool::Pan => {}
                }
            }
        }

        // -------- Draw ------------------------------------------------

        if let Some(tex) = &app.tex {
            let r = Rect::from_min_size(to_s(Pos2::ZERO), app.img_size * zoom);
            painter.image(
                tex.id(),
                r,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }

        // Fall back to 1 px/m if scale isn't set yet — only matters for the
        // draft road preview, which can't be sized correctly without it.
        let ppm = app.px_per_m.unwrap_or(1.0);

        // Finished roads: tessellate each one, transform, draw. Road width
        // rescales automatically because it's derived from `ppm * zoom`.
        for road in &app.roads {
            let smooth_img = spline::catmull_rom(&road.pts);
            let pts_screen: Vec<Pos2> = smooth_img.iter().map(|&p| to_s(p)).collect();
            let w = road.lane_m * road.lanes as f32 * ppm * zoom;
            paint_road(&painter, &pts_screen, w, road.lanes, road.center);
        }

        // Road in progress, with a live preview segment to the cursor.
        // Tessellating this too means the preview curves as you draw — a
        // huge usability win because it shows the shape you'll actually get.
        if !app.current.is_empty() {
            let mut ctrl: Vec<Pos2> = app.current.clone();
            if let Some(h) = resp.hover_pos() {
                ctrl.push(to_i(h));
            }
            let smooth_img = spline::catmull_rom(&ctrl);
            let pts_screen: Vec<Pos2> = smooth_img.iter().map(|&p| to_s(p)).collect();
            let w = app.lane_m * app.lanes as f32 * ppm * zoom;
            paint_road(&painter, &pts_screen, w, app.lanes, app.center_style);
        }

        // Control-point handles. Drawn *after* the roads so they sit on top,
        // and only for the Road tool so they don't clutter Pan/Calibrate.
        if app.tool == Tool::Road {
            paint_handles(app, &painter, &to_s);
        }

        // Calibration markers.
        if !app.calib_pts.is_empty() {
            let red = Color32::from_rgb(235, 60, 60);
            let pts: Vec<Pos2> = app.calib_pts.iter().map(|&p| to_s(p)).collect();
            if pts.len() == 2 {
                painter.line_segment([pts[0], pts[1]], Stroke::new(2.0_f32, red));
            }
            for q in pts {
                painter.circle_stroke(q, 6.0, Stroke::new(2.0_f32, red));
            }
        }

        if !app.attribution.is_empty() {
            paint_attribution(&painter, rect, &app.attribution);
        }
    });
}

/// Paints the handle dots for every editable vertex, highlighting whichever
/// one the cursor is currently over.
///
/// We pass `to_s` (image -> screen) as a closure so this function doesn't
/// need to know anything about the current view transform.
fn paint_handles(app: &App, painter: &egui::Painter, to_s: &impl Fn(Pos2) -> Pos2) {
    // The base handle colour is a light grey so it reads against both dark
    // asphalt and typical satellite imagery.
    let base = Color32::from_rgb(220, 220, 220);
    let hover = Color32::from_rgb(255, 210, 80);

    // Draft road first, so its handle wins the highlight if a committed
    // handle happens to sit at the same location.
    for (i, &p) in app.current.iter().enumerate() {
        let c = if app.hover_vertex == Some(VertexRef::Draft(i)) {
            hover
        } else {
            base
        };
        let s = to_s(p);
        painter.circle_filled(s, HANDLE_RADIUS, c);
        painter.circle_stroke(
            s,
            HANDLE_RADIUS,
            Stroke::new(1.0_f32, Color32::from_black_alpha(160)),
        );
    }

    for (ri, road) in app.roads.iter().enumerate() {
        for (vi, &p) in road.pts.iter().enumerate() {
            let c = if app.hover_vertex == Some(VertexRef::Committed(ri, vi)) {
                hover
            } else {
                base
            };
            let s = to_s(p);
            painter.circle_filled(s, HANDLE_RADIUS, c);
            painter.circle_stroke(
                s,
                HANDLE_RADIUS,
                Stroke::new(1.0_f32, Color32::from_black_alpha(160)),
            );
        }
    }
}

/// Returns the closest editable vertex to `img_pt` within `radius`, expressed
/// in image pixels.
///
/// The in-progress road is tested first so that, on a tie, its vertices win —
/// the user is more likely to want to adjust a point they just placed than
/// one buried under an older road.
fn find_vertex(app: &App, img_pt: Pos2, radius: f32) -> Option<VertexRef> {
    let r2 = radius * radius;
    let mut best: Option<(VertexRef, f32)> = None;

    let mut consider = |vr: VertexRef, p: Pos2| {
        let d2 = (p - img_pt).length_sq();
        if d2 <= r2 {
            // Strictly-less comparison keeps the first-considered vertex on
            // ties, which is exactly what we want for the draft-vs-committed
            // precedence described above.
            match best {
                Some((_, bd2)) if bd2 <= d2 => {}
                _ => best = Some((vr, d2)),
            }
        }
    };

    for (vi, &p) in app.current.iter().enumerate() {
        consider(VertexRef::Draft(vi), p);
    }
    for (ri, road) in app.roads.iter().enumerate() {
        for (vi, &p) in road.pts.iter().enumerate() {
            consider(VertexRef::Committed(ri, vi), p);
        }
    }

    best.map(|(vr, _)| vr)
}

/// Moves the vertex referenced by `v` by `delta` image-space pixels.
///
/// Doing it as a delta (rather than snapping to the cursor) means the motion
/// tracks the mouse 1:1 even if the user grabs a handle slightly off-center.
fn shift_vertex(app: &mut App, v: VertexRef, delta: Vec2) {
    // Split the two cases so the borrow checker can see that only one of
    // `app.current` and `app.roads` is touched at a time.
    let pts: &mut Vec<Pos2> = match v {
        VertexRef::Draft(_) => &mut app.current,
        VertexRef::Committed(ri, _) => match app.roads.get_mut(ri) {
            Some(road) => &mut road.pts,
            None => return,
        },
    };
    let vi = match v {
        VertexRef::Draft(vi) => vi,
        VertexRef::Committed(_, vi) => vi,
    };
    if let Some(p) = pts.get_mut(vi) {
        *p += delta;
    }
}

/// Removes a control point. A committed road that drops below two points is
/// removed entirely — one point is not a road.
fn delete_vertex(app: &mut App, v: VertexRef) {
    match v {
        VertexRef::Draft(vi) => {
            if vi < app.current.len() {
                app.current.remove(vi);
            }
        }
        VertexRef::Committed(ri, vi) => {
            // Decide whether to remove the road *inside* the same borrow that
            // mutates it, then act on that decision after the borrow ends.
            let remove_road = {
                let Some(road) = app.roads.get_mut(ri) else {
                    return;
                };
                if vi < road.pts.len() {
                    road.pts.remove(vi);
                }
                road.pts.len() < 2
            };
            if remove_road {
                app.roads.remove(ri);
            }
        }
    }
}