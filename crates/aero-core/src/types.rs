//! Coordinate-space primitives. Intersection is not frame projection or DPR scaling.
//!
//! Screenshot pixels cannot be passed to a root-CSS input API:
//!
//! ```compile_fail,E0308
//! use aero_core::types::{RootViewportPoint, ScreenshotPixelPoint};
//! fn needs_root_css(_: RootViewportPoint) {}
//! let pixel = ScreenshotPixelPoint::new(10.0, 20.0).unwrap();
//! needs_root_css(pixel);
//! ```
//!
//! A frame-local rectangle requires an explicit boundary to access its raw values:
//!
//! ```compile_fail,E0308
//! use aero_core::types::{CssRect, FrameLocalRect};
//! fn needs_raw_rect(_: CssRect) {}
//! let local = FrameLocalRect::new(CssRect {
//!     x: 0.0, y: 0.0, width: 10.0, height: 20.0,
//! }).unwrap();
//! needs_raw_rect(local);
//! ```

/// An unvalidated axis-aligned rectangle in CSS pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CssRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl CssRect {
    /// Return a visible interior point in the same coordinate space as this rect.
    /// Invalid/unrepresentable bounds or an empty intersection return `None`.
    pub fn clipped_center(self, viewport: (f64, f64)) -> Option<(f64, f64)> {
        let (raw_right, raw_bottom) = self.valid_bounds()?;
        if !viewport.0.is_finite()
            || !viewport.1.is_finite()
            || viewport.0 <= 0.0
            || viewport.1 <= 0.0
        {
            return None;
        }
        let left = self.x.max(0.0);
        let top = self.y.max(0.0);
        let right = raw_right.min(viewport.0);
        let bottom = raw_bottom.min(viewport.1);
        if right <= left || bottom <= top {
            return None;
        }
        let x = left + (right - left) / 2.0;
        let y = top + (bottom - top) / 2.0;
        // Very small spans can round a midpoint onto an edge. Do not turn that
        // into a click on the wrong side of the visible target boundary.
        (x > left && x < right && y > top && y < bottom).then_some((x, y))
    }

    fn valid_bounds(self) -> Option<(f64, f64)> {
        if ![self.x, self.y, self.width, self.height]
            .iter()
            .all(|value| value.is_finite())
            || self.width <= 0.0
            || self.height <= 0.0
        {
            return None;
        }
        let right = self.x + self.width;
        let bottom = self.y + self.height;
        (right.is_finite() && bottom.is_finite() && right > self.x && bottom > self.y)
            .then_some((right, bottom))
    }
}

/// A frame-local rectangle; no automatic parent/root coordinate conversion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameLocalRect(CssRect);

impl FrameLocalRect {
    /// Validate finite, positive, representable bounds, allowing negative offsets.
    pub fn new(rect: CssRect) -> Option<Self> {
        rect.valid_bounds().map(|_| Self(rect))
    }

    /// Read raw local CSS values without projecting to a parent or root frame.
    pub fn rect(self) -> CssRect {
        self.0
    }
}

macro_rules! viewport_point {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub struct $name {
            x: f64,
            y: f64,
        }

        impl $name {
            /// Require finite, nonnegative coordinates. Current viewport bounds,
            /// target ownership and hit testing remain the caller's responsibility.
            pub fn new(x: f64, y: f64) -> Option<Self> {
                (x.is_finite() && y.is_finite() && x >= 0.0 && y >= 0.0).then_some(Self { x, y })
            }

            /// Read values in this point's original units, without conversion.
            pub fn coordinates(self) -> (f64, f64) {
                (self.x, self.y)
            }
        }
    };
}

viewport_point!(
    RootViewportPoint,
    "A root-viewport point in CSS pixels, not screenshot pixels."
);
viewport_point!(
    ScreenshotPixelPoint,
    "A screenshot-pixel point; fractional values are preserved."
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_uses_visible_intersection() {
        let cases = [
            ((-100.0, 20.0, 120.0, 40.0), (10.0, 40.0)),
            ((0.0, 0.0, 120.0, 80.0), (60.0, 40.0)),
            ((-20.0, -20.0, 40.0, 40.0), (10.0, 10.0)),
            ((780.0, 580.0, 40.0, 40.0), (790.0, 590.0)),
            ((-20.0, -20.0, 1640.0, 1240.0), (400.0, 300.0)),
            ((20.5, 10.25, 1.5, 2.5), (21.25, 11.5)),
        ];
        for ((x, y, width, height), expected) in cases {
            let rect = CssRect {
                x,
                y,
                width,
                height,
            };
            assert_eq!(
                rect.clipped_center((800.0, 600.0)),
                Some(expected),
                "{rect:?}"
            );
        }
    }

    #[test]
    fn center_rejects_nonfinite_inputs() {
        for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            for field in 0..6 {
                let mut values = [20.0, 20.0, 40.0, 40.0, 800.0, 600.0];
                values[field] = invalid;
                let [x, y, width, height, viewport_width, viewport_height] = values;
                let rect = CssRect {
                    x,
                    y,
                    width,
                    height,
                };
                assert_eq!(rect.clipped_center((viewport_width, viewport_height)), None);
            }
        }
    }

    #[test]
    fn center_rejects_nonpositive_dimensions() {
        for invalid in [0.0, -1.0] {
            for field in 2..6 {
                let mut values = [20.0, 20.0, 40.0, 40.0, 800.0, 600.0];
                values[field] = invalid;
                let [x, y, width, height, viewport_width, viewport_height] = values;
                let rect = CssRect {
                    x,
                    y,
                    width,
                    height,
                };
                assert_eq!(rect.clipped_center((viewport_width, viewport_height)), None);
            }
        }
    }

    #[test]
    fn center_rejects_offscreen_or_zero_visible_area() {
        for (x, y) in [
            (800.0, 0.0),
            (0.0, 600.0),
            (-40.0, 0.0),
            (0.0, -40.0),
            (900.0, 700.0),
        ] {
            let rect = CssRect {
                x,
                y,
                width: 40.0,
                height: 40.0,
            };
            assert_eq!(rect.clipped_center((800.0, 600.0)), None);
        }
    }

    #[test]
    fn center_rejects_overflow_and_keeps_finite_midpoints() {
        let overflowing = CssRect {
            x: f64::MAX,
            y: 0.0,
            width: f64::MAX,
            height: 10.0,
        };
        assert_eq!(overflowing.clipped_center((f64::MAX, 100.0)), None);
        let bottom_overflow = CssRect {
            x: 0.0,
            y: f64::MAX,
            width: 10.0,
            height: f64::MAX,
        };
        assert_eq!(bottom_overflow.clipped_center((100.0, f64::MAX)), None);
        let large = CssRect {
            x: 1.0e308,
            y: 0.0,
            width: 4.0e307,
            height: 10.0,
        };
        let (x, y) = large.clipped_center((f64::MAX, 100.0)).unwrap();
        assert!(x.is_finite() && y.is_finite());
        assert!(x > large.x && x < large.x + large.width);
    }

    #[test]
    fn center_rejects_unrepresentable_interior() {
        let smallest = f64::from_bits(1);
        for x in [0.0, smallest] {
            let rect = CssRect {
                x,
                y: 0.0,
                width: smallest,
                height: 1.0,
            };
            assert_eq!(rect.clipped_center((1.0, 1.0)), None);
        }
    }

    #[test]
    fn center_keeps_small_representable_intersections() {
        let size = f64::MIN_POSITIVE;
        let rect = CssRect {
            x: 0.0,
            y: 0.0,
            width: size,
            height: size,
        };
        let (x, y) = rect.clipped_center((1.0, 1.0)).unwrap();
        assert!(x > 0.0 && x < size && y > 0.0 && y < size);
    }

    #[test]
    fn coordinate_spaces_are_distinct() {
        assert_ne!(
            std::any::TypeId::of::<RootViewportPoint>(),
            std::any::TypeId::of::<ScreenshotPixelPoint>()
        );
        assert_ne!(
            std::any::TypeId::of::<FrameLocalRect>(),
            std::any::TypeId::of::<CssRect>()
        );
    }

    #[test]
    fn coordinate_points_preserve_fractional_values() {
        let css = RootViewportPoint::new(10.25, 20.5).unwrap();
        let pixel = ScreenshotPixelPoint::new(10.25, 20.5).unwrap();
        assert_eq!(css.coordinates(), (10.25, 20.5));
        assert_eq!(pixel.coordinates(), (10.25, 20.5));
        assert_eq!(
            RootViewportPoint::new(0.0, 0.0).unwrap().coordinates(),
            (0.0, 0.0)
        );
    }

    #[test]
    fn coordinate_points_reject_nonfinite_or_negative_values() {
        for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
            assert!(RootViewportPoint::new(invalid, 0.0).is_none());
            assert!(RootViewportPoint::new(0.0, invalid).is_none());
            assert!(ScreenshotPixelPoint::new(invalid, 0.0).is_none());
            assert!(ScreenshotPixelPoint::new(0.0, invalid).is_none());
        }
    }

    #[test]
    fn frame_local_rect_keeps_valid_offsets_without_projecting() {
        let rect = CssRect {
            x: -100.0,
            y: 20.0,
            width: 120.0,
            height: 40.0,
        };
        assert_eq!(FrameLocalRect::new(rect).unwrap().rect(), rect);
    }

    #[test]
    fn frame_local_rect_rejects_invalid_or_unrepresentable_bounds() {
        for rect in [
            CssRect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 1.0,
            },
            CssRect {
                x: f64::NAN,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            CssRect {
                x: f64::MAX,
                y: 0.0,
                width: f64::MAX,
                height: 1.0,
            },
            CssRect {
                x: 1.0e308,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
        ] {
            assert!(FrameLocalRect::new(rect).is_none(), "{rect:?}");
        }
    }
}
