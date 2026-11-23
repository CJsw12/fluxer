//! Presence status indicator (`StatusIndicator.tsx` + the `flx-mask-presence-*` masks from
//! `scripts/GenerateAvatarMasks.ts`). Shapes in unit coordinates, as in the official masks:
//! online = circle(0.5,0.5,r .5); idle = that minus circle(.25,.25,r .375); dnd = that minus
//! rect(x .125, y .375, w .75, h .25, rx .125); offline/invisible = that minus circle(.5,.5,r .25).

use std::f32::consts::{PI, TAU};

use fluxer_client_core::state::Status;
use gpui_kit::{Hsla, PathBuilder, Pixels, Point, Window, canvas, prelude::*, px};

use crate::theme::Theme;

const SEGMENTS: usize = 48;

/// Theme color of a presence status.
pub fn color(status: Status, theme: &Theme) -> Hsla {
    match status {
        Status::Online => theme.status_online,
        Status::Idle => theme.status_idle,
        Status::Dnd => theme.status_dnd,
        Status::Invisible | Status::Offline => theme.status_offline,
    }
}

fn arc(cx: f32, cy: f32, r: f32, from: f32, to: f32) -> Vec<(f32, f32)> {
    (0..=SEGMENTS)
        .map(|i| {
            let a = from + (to - from) * i as f32 / SEGMENTS as f32;
            (cx + r * a.cos(), cy + r * a.sin())
        })
        .collect()
}

fn circle(cx: f32, cy: f32, r: f32, reversed: bool) -> Vec<(f32, f32)> {
    let mut pts = arc(cx, cy, r, 0., TAU);
    if reversed {
        pts.reverse();
    }
    pts
}

/// Polygons (unit square) whose union with even-odd/non-zero fill is the status shape.
fn shape(status: Status) -> Vec<Vec<(f32, f32)>> {
    match status {
        Status::Online => vec![circle(0.5, 0.5, 0.5, false)],
        Status::Idle => {
            // crescent: main circle minus circle (.25,.25) r .375
            let (big_r, cut_r) = (0.5_f32, 0.375_f32);
            let d = 0.25_f32 * 2_f32.sqrt();
            let alpha = ((big_r * big_r + d * d - cut_r * cut_r) / (2. * big_r * d)).clamp(-1., 1.).acos();
            let beta = ((d * d + cut_r * cut_r - big_r * big_r) / (2. * d * cut_r)).clamp(-1., 1.).acos();
            let phi = 5. * PI / 4.; // direction main centre -> cutout centre (y down)
            let mut pts = arc(0.5, 0.5, big_r, phi + alpha, phi + TAU - alpha);
            let mut inner = arc(0.25, 0.25, cut_r, phi - PI - beta, phi - PI + beta);
            let last = pts.last().copied().unwrap_or((0., 0.));
            let near = |p: (f32, f32)| (p.0 - last.0).powi(2) + (p.1 - last.1).powi(2);
            if let (Some(a), Some(b)) = (inner.first().copied(), inner.last().copied())
                && near(b) < near(a)
            {
                inner.reverse();
            }
            pts.extend(inner);
            vec![pts]
        }
        Status::Dnd => {
            // bar x .125..0.875, y .375..0.625, rx .125 (a stadium), wound opposite to the circle
            let mut bar = arc(0.75, 0.5, 0.125, -PI / 2., PI / 2.);
            bar.extend(arc(0.25, 0.5, 0.125, PI / 2., 3. * PI / 2.));
            bar.reverse();
            vec![circle(0.5, 0.5, 0.5, false), bar]
        }
        Status::Invisible | Status::Offline => vec![circle(0.5, 0.5, 0.5, false), circle(0.5, 0.5, 0.25, true)],
    }
}

/// A `size`px status indicator filled with `fill`.
pub fn status_indicator(status: Status, size: f32, fill: Hsla) -> impl IntoElement {
    let polys = shape(status);
    canvas(
        |_, _, _| (),
        move |bounds, (), window: &mut Window, _| {
            let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
            let at = |p: (f32, f32)| -> Point<Pixels> { bounds.origin + gpui_kit::point(px(p.0 * w), px(p.1 * h)) };
            let mut b = PathBuilder::fill();
            for poly in &polys {
                let mut it = poly.iter().copied();
                let Some(first) = it.next() else { continue };
                b.move_to(at(first));
                for p in it {
                    b.line_to(at(p));
                }
                b.close();
            }
            if let Ok(path) = b.build() {
                window.paint_path(path, fill);
            }
        },
    )
    .flex_none()
    .size(px(size))
}

/// Indicator using the theme color for `status`.
pub fn themed(status: Status, size: f32, theme: &Theme) -> impl IntoElement {
    status_indicator(status, size, color(status, theme))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_stay_inside_the_unit_square() {
        for s in [Status::Online, Status::Idle, Status::Dnd, Status::Offline] {
            for poly in shape(s) {
                assert!(!poly.is_empty());
                for (x, y) in poly {
                    assert!((-0.001..=1.001).contains(&x) && (-0.001..=1.001).contains(&y), "{s:?} {x} {y}");
                }
            }
        }
    }
}
