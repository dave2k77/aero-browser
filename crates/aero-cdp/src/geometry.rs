//! D05: pure frame geometry. Maps a target box measured in its frame's
//! session to one root-viewport point, and back to a point in every frame on
//! the path for per-level hit tests.
//!
//! Protocol facts this relies on (D05 spike, Chrome 156):
//! - `DOM.getContentQuads` and `DOM.getBoxModel` answer in the coordinates of
//!   the calling session's local root: the root viewport for the page session
//!   (including same-process child frames), an OOPIF's own viewport for its
//!   session.
//! - An OOPIF point maps into its parent's session as
//!   `owner content origin + point × (owner content size ÷ child inner size)`.
//!
//! Every input must already be axis-aligned; [`axis_aligned`] turns a quad
//! into a rectangle or refuses it (SPEC-19).

use aero_core::types::CssRect;

/// How far quad corners may deviate and still count as axis-aligned (CSS px).
pub const AXIS_TOLERANCE: f64 = 0.01;

/// Where a child frame sits in its parent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameLink {
    /// The owner `<iframe>`'s content box, in the coordinates of the parent
    /// frame's session.
    pub owner_content: CssRect,
    /// The child's layout viewport including scrollbars (`innerWidth`,
    /// `innerHeight`): what the owner's content box displays.
    pub inner: (f64, f64),
    /// The child is its session's local root (an out-of-process frame), so
    /// its own coordinates are its session's.
    pub local_root: bool,
}

/// One frame on the path from the target's frame to the main frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameLevel {
    /// The visible viewport in the frame's own CSS pixels (no scrollbars).
    pub viewport: (f64, f64),
    /// `None` for the main frame, which must be last.
    pub link: Option<FrameLink>,
}

/// A point to press and its position in each frame on the path.
#[derive(Clone, Debug, PartialEq)]
pub struct FramePoint {
    /// Root-viewport CSS pixels: where to dispatch input.
    pub root: (f64, f64),
    /// Frame-local CSS pixels, one per path level, target frame first.
    pub local: Vec<(f64, f64)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameGeometryError {
    /// The path or a measurement is malformed (non-finite, non-positive
    /// sizes, main frame not last).
    Invalid,
    /// The target has no box with an area.
    NotRendered,
    /// No part of the target is visible through every frame viewport.
    OffViewport,
}

/// The rectangle a quad describes, if its four points are the corners of an
/// axis-aligned rectangle (in any order).
pub fn axis_aligned(corners: &[f64; 8]) -> Option<CssRect> {
    let xs = [corners[0], corners[2], corners[4], corners[6]];
    let ys = [corners[1], corners[3], corners[5], corners[7]];
    let min_x = xs.iter().copied().fold(f64::INFINITY, f64::min);
    let max_x = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let min_y = ys.iter().copied().fold(f64::INFINITY, f64::min);
    let max_y = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let near = |a: f64, b: f64| (a - b).abs() <= AXIS_TOLERANCE;
    let on_corner = (0..4).all(|i| {
        (near(xs[i], min_x) || near(xs[i], max_x)) && (near(ys[i], min_y) || near(ys[i], max_y))
    });
    let has = |x: f64, y: f64| (0..4).any(|i| near(xs[i], x) && near(ys[i], y));
    let complete = has(min_x, min_y) && has(max_x, min_y) && has(max_x, max_y) && has(min_x, max_y);
    (on_corner && complete).then_some(CssRect {
        x: min_x,
        y: min_y,
        width: max_x - min_x,
        height: max_y - min_y,
    })
}

/// `local → parent` per axis: `parent = origin + local × scale`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Affine {
    origin: (f64, f64),
    scale: (f64, f64),
}

impl Affine {
    fn point(self, (x, y): (f64, f64)) -> (f64, f64) {
        (
            self.origin.0 + x * self.scale.0,
            self.origin.1 + y * self.scale.1,
        )
    }

    fn inverse_point(self, (x, y): (f64, f64)) -> (f64, f64) {
        (
            (x - self.origin.0) / self.scale.0,
            (y - self.origin.1) / self.scale.1,
        )
    }

    fn rect(self, rect: CssRect) -> CssRect {
        let (x, y) = self.point((rect.x, rect.y));
        CssRect {
            x,
            y,
            width: rect.width * self.scale.0,
            height: rect.height * self.scale.1,
        }
    }

    fn inverse_rect(self, rect: CssRect) -> CssRect {
        let (x, y) = self.inverse_point((rect.x, rect.y));
        CssRect {
            x,
            y,
            width: rect.width / self.scale.0,
            height: rect.height / self.scale.1,
        }
    }
}

/// The child's local coordinates into its owner's session coordinates.
fn to_owner_session(link: &FrameLink) -> Affine {
    Affine {
        origin: (link.owner_content.x, link.owner_content.y),
        scale: (
            link.owner_content.width / link.inner.0,
            link.owner_content.height / link.inner.1,
        ),
    }
}

/// Frame-local coordinates of `level` into its parent's local coordinates.
/// A parent that is not a local root shares its session with its own
/// parent, so the child's session coordinates are first taken back into the
/// parent's local ones.
fn to_parent(level: &FrameLevel, parent: &FrameLevel) -> Affine {
    let child = to_owner_session(level.link.as_ref().expect("checked: not the main frame"));
    match &parent.link {
        Some(link) if !link.local_root => {
            let parent = to_owner_session(link);
            Affine {
                origin: (
                    (child.origin.0 - parent.origin.0) / parent.scale.0,
                    (child.origin.1 - parent.origin.1) / parent.scale.1,
                ),
                scale: (
                    child.scale.0 / parent.scale.0,
                    child.scale.1 / parent.scale.1,
                ),
            }
        }
        _ => child,
    }
}

fn finite_positive(value: f64) -> bool {
    value.is_finite() && value > 0.0
}

fn valid_path(path: &[FrameLevel]) -> Result<(), FrameGeometryError> {
    let Some((main, children)) = path.split_last() else {
        return Err(FrameGeometryError::Invalid);
    };
    if main.link.is_some() || children.iter().any(|level| level.link.is_none()) {
        return Err(FrameGeometryError::Invalid);
    }
    for level in path {
        if !finite_positive(level.viewport.0) || !finite_positive(level.viewport.1) {
            return Err(FrameGeometryError::Invalid);
        }
        if let Some(link) = &level.link {
            let owner = link.owner_content;
            if !finite_positive(link.inner.0)
                || !finite_positive(link.inner.1)
                || ![owner.x, owner.y, owner.width, owner.height]
                    .iter()
                    .all(|value| value.is_finite())
                || owner.width < 0.0
                || owner.height < 0.0
            {
                return Err(FrameGeometryError::Invalid);
            }
        }
    }
    if children.iter().any(|level| {
        let owner = level.link.as_ref().map(|link| link.owner_content);
        owner.is_some_and(|owner| owner.width == 0.0 || owner.height == 0.0)
    }) {
        return Err(FrameGeometryError::NotRendered);
    }
    Ok(())
}

/// `rect` ∩ `[0, width] × [0, height]`, if that has an area.
fn clip(rect: CssRect, (width, height): (f64, f64)) -> Option<CssRect> {
    let left = rect.x.max(0.0);
    let top = rect.y.max(0.0);
    let right = (rect.x + rect.width).min(width);
    let bottom = (rect.y + rect.height).min(height);
    (right > left && bottom > top).then_some(CssRect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
}

/// Choose the centre of the first target fragment visible through every
/// frame on `path`. `fragments` are in the coordinates of the target frame's
/// session; `path` starts at the target's frame and ends at the main frame.
pub fn frame_point(
    fragments: &[CssRect],
    path: &[FrameLevel],
) -> Result<FramePoint, FrameGeometryError> {
    valid_path(path)?;
    let fragments: Vec<CssRect> = fragments
        .iter()
        .copied()
        .filter(|rect| {
            [rect.x, rect.y, rect.width, rect.height]
                .iter()
                .all(|value| value.is_finite())
                && rect.width > 0.0
                && rect.height > 0.0
        })
        .collect();
    if fragments.is_empty() {
        return Err(FrameGeometryError::NotRendered);
    }
    let up: Vec<Affine> = path
        .windows(2)
        .map(|pair| to_parent(&pair[0], &pair[1]))
        .collect();
    // Fragments are in the target session's coordinates; a target frame that
    // is not its session's local root needs them taken into its own.
    let into_target = match &path[0].link {
        Some(link) if !link.local_root => Some(to_owner_session(link)),
        _ => None,
    };
    let (main, children) = path.split_last().expect("checked: non-empty");
    for fragment in fragments {
        let mut visible = Some(match into_target {
            Some(affine) => affine.inverse_rect(fragment),
            None => fragment,
        });
        for (level, affine) in children.iter().zip(&up) {
            visible = visible
                .and_then(|rect| clip(rect, level.viewport))
                .map(|rect| affine.rect(rect));
        }
        let Some(root) = visible.and_then(|rect| rect.clipped_center(main.viewport)) else {
            continue;
        };
        let mut local = vec![root];
        for affine in up.iter().rev() {
            let below = affine.inverse_point(*local.last().expect("non-empty"));
            local.push(below);
        }
        local.reverse();
        return Ok(FramePoint { root, local });
    }
    Err(FrameGeometryError::OffViewport)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> CssRect {
        CssRect {
            x,
            y,
            width,
            height,
        }
    }

    const ROOT: FrameLevel = FrameLevel {
        viewport: (1265.0, 800.0),
        link: None,
    };

    fn level(
        viewport: (f64, f64),
        owner: CssRect,
        inner: (f64, f64),
        local_root: bool,
    ) -> FrameLevel {
        FrameLevel {
            viewport,
            link: Some(FrameLink {
                owner_content: owner,
                inner,
                local_root,
            }),
        }
    }

    #[test]
    fn frame_point_in_the_main_frame_is_the_clipped_centre() {
        let point = frame_point(&[rect(8.0, 588.0, 79.0, 21.0)], &[ROOT]).unwrap();
        assert_eq!(point.root, (47.5, 598.5));
        assert_eq!(point.local, vec![(47.5, 598.5)]);
    }

    #[test]
    fn frame_point_same_process_quads_are_already_root_coordinates() {
        // Spike: a same-process frame (border 7, padding 5) scrolled 50 px;
        // its button reported at (82, 242) 80×30 on the page session.
        let same = level(
            (300.0, 150.0),
            rect(52.0, 192.0, 300.0, 150.0),
            (300.0, 150.0),
            false,
        );
        let point = frame_point(&[rect(82.0, 242.0, 80.0, 30.0)], &[same, ROOT]).unwrap();
        assert_eq!(point.root, (122.0, 257.0));
        assert_eq!(point.local, vec![(70.0, 65.0), (122.0, 257.0)]);
        // A same-process scale(0.5) frame: quads already scaled.
        let scaled = level(
            (300.0, 150.0),
            rect(422.0, 182.0, 150.0, 75.0),
            (300.0, 150.0),
            false,
        );
        let point = frame_point(&[rect(437.0, 207.0, 40.0, 15.0)], &[scaled, ROOT]).unwrap();
        assert_eq!(point.root, (457.0, 214.5));
        assert_eq!(point.local[0], (70.0, 65.0));
    }

    #[test]
    fn frame_point_maps_oopifs_through_owner_content_boxes() {
        // Spike: cross-site frame, owner content at (52, 492) 420×300.
        let cross = level(
            (405.0, 285.0),
            rect(52.0, 492.0, 420.0, 300.0),
            (420.0, 300.0),
            true,
        );
        let point = frame_point(&[rect(50.0, 90.0, 80.0, 30.0)], &[cross, ROOT]).unwrap();
        assert_eq!(point.root, (142.0, 597.0));
        assert_eq!(point.local, vec![(90.0, 105.0), (142.0, 597.0)]);
        // A cross-site scale(0.5) frame.
        let scaled = level(
            (300.0, 200.0),
            rect(524.0, 684.0, 150.0, 100.0),
            (300.0, 200.0),
            true,
        );
        let point = frame_point(&[rect(30.0, 100.0, 80.0, 30.0)], &[scaled, ROOT]).unwrap();
        assert_eq!(point.root, (559.0, 741.5));
        assert_eq!(point.local[0], (70.0, 115.0));
        // A nested OOPIF: its owner box is in the parent OOPIF's viewport.
        let leaf = level(
            (185.0, 200.0),
            rect(206.0, -24.0, 200.0, 200.0),
            (200.0, 200.0),
            true,
        );
        let point = frame_point(&[rect(30.0, 100.0, 80.0, 30.0)], &[leaf, cross, ROOT]).unwrap();
        assert_eq!(point.root, (328.0, 583.0));
        assert_eq!(
            point.local,
            vec![(70.0, 115.0), (276.0, 91.0), (328.0, 583.0)]
        );
    }

    #[test]
    fn frame_point_composes_nested_same_process_scales() {
        // Outer same-process frame at scale 0.5, inner one inside it at a
        // further 0.5. Session coordinates are root coordinates, so the root
        // point is the fragment's centre; the local points undo the scales.
        let outer = level(
            (400.0, 400.0),
            rect(100.0, 100.0, 200.0, 200.0),
            (400.0, 400.0),
            false,
        );
        let inner = level(
            (200.0, 200.0),
            rect(150.0, 150.0, 50.0, 50.0),
            (200.0, 200.0),
            false,
        );
        let point = frame_point(&[rect(160.0, 160.0, 10.0, 10.0)], &[inner, outer, ROOT]).unwrap();
        assert_eq!(point.root, (165.0, 165.0));
        assert_eq!(
            point.local,
            vec![(60.0, 60.0), (130.0, 130.0), (165.0, 165.0)]
        );
    }

    #[test]
    fn frame_point_clips_at_every_level() {
        let cross = level(
            (405.0, 285.0),
            rect(52.0, 492.0, 420.0, 300.0),
            (420.0, 300.0),
            true,
        );
        // Half outside the frame's right edge: centre of the visible half.
        let point = frame_point(&[rect(385.0, 10.0, 40.0, 20.0)], &[cross, ROOT]).unwrap();
        assert_eq!(point.local[0], (395.0, 20.0));
        // Wholly outside the frame viewport (but inside the root viewport
        // once mapped): not visible.
        assert_eq!(
            frame_point(&[rect(500.0, 10.0, 40.0, 20.0)], &[cross, ROOT]),
            Err(FrameGeometryError::OffViewport)
        );
        // Inside its frame, but the frame itself is below the root viewport.
        let low = level(
            (405.0, 285.0),
            rect(52.0, 900.0, 420.0, 300.0),
            (420.0, 300.0),
            true,
        );
        assert_eq!(
            frame_point(&[rect(10.0, 10.0, 40.0, 20.0)], &[low, ROOT]),
            Err(FrameGeometryError::OffViewport)
        );
        // The first fragment is clipped away; the second is used.
        let point = frame_point(
            &[rect(10.0, -40.0, 40.0, 20.0), rect(10.0, 10.0, 40.0, 20.0)],
            &[cross, ROOT],
        )
        .unwrap();
        assert_eq!(point.local[0], (30.0, 20.0));
    }

    #[test]
    fn frame_point_refuses_empty_and_malformed_input() {
        let cross = level(
            (405.0, 285.0),
            rect(52.0, 492.0, 420.0, 300.0),
            (420.0, 300.0),
            true,
        );
        assert_eq!(
            frame_point(&[], &[cross, ROOT]),
            Err(FrameGeometryError::NotRendered)
        );
        assert_eq!(
            frame_point(&[rect(1.0, 1.0, 0.0, 5.0)], &[cross, ROOT]),
            Err(FrameGeometryError::NotRendered)
        );
        let fragment = [rect(10.0, 10.0, 40.0, 20.0)];
        // No main frame, main frame not last, empty path.
        assert_eq!(
            frame_point(&fragment, &[cross]),
            Err(FrameGeometryError::Invalid)
        );
        assert_eq!(
            frame_point(&fragment, &[ROOT, cross]),
            Err(FrameGeometryError::Invalid)
        );
        assert_eq!(
            frame_point(&fragment, &[]),
            Err(FrameGeometryError::Invalid)
        );
        // Zero or non-finite sizes.
        let zero = level(
            (405.0, 285.0),
            rect(52.0, 492.0, 420.0, 300.0),
            (0.0, 300.0),
            true,
        );
        assert_eq!(
            frame_point(&fragment, &[zero, ROOT]),
            Err(FrameGeometryError::Invalid)
        );
        let nan = level(
            (f64::NAN, 285.0),
            rect(52.0, 492.0, 420.0, 300.0),
            (420.0, 300.0),
            true,
        );
        assert_eq!(
            frame_point(&fragment, &[nan, ROOT]),
            Err(FrameGeometryError::Invalid)
        );
        // A collapsed owner box shows nothing.
        let collapsed = level(
            (405.0, 285.0),
            rect(52.0, 492.0, 0.0, 300.0),
            (420.0, 300.0),
            true,
        );
        assert_eq!(
            frame_point(&fragment, &[collapsed, ROOT]),
            Err(FrameGeometryError::NotRendered)
        );
    }

    #[test]
    fn axis_aligned_accepts_rectangles_and_refuses_rotation() {
        assert_eq!(
            axis_aligned(&[87.0, 609.0, 8.0, 609.0, 8.0, 588.0, 87.0, 588.0]),
            Some(rect(8.0, 588.0, 79.0, 21.0))
        );
        // Spike: a rotate(10deg) iframe's content quad.
        assert_eq!(
            axis_aligned(&[
                815.3, 155.09, 1110.74, 207.19, 1084.7, 354.91, 789.26, 302.81
            ]),
            None
        );
    }
}
