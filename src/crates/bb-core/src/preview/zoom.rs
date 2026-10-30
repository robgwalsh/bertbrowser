//! The arithmetic behind the preview pane's zoomable image, one axis at a time.
//!
//! The image is laid out fitted to the pane, then drawn through `p * scale + offset` in its own
//! coordinates, so a scale of 1 is "fit", and everything here is relative to that. The view owns
//! the transform; this owns the rules, so they can be tested without a window.
//! Spec: Core/Services/Preview/ImageZoom.cs.

pub struct ImageZoom;

impl ImageZoom {
    /// How far past "fit" the wheel will go. Enough to count the pixels of a photo shown in a
    /// narrow pane.
    pub const MAX_SCALE: f64 = 64.0;

    /// One wheel notch multiplies or divides the scale by this.
    pub const STEP_FACTOR: f64 = 1.2;

    /// The scale after `notches` wheel notches (positive zooms in). Never below `min_scale`
    /// (normally 1, fit, since zooming out past the pane only shrinks the picture into a margin)
    /// and never above `MAX_SCALE`.
    pub fn step(scale: f64, notches: f64, min_scale: f64) -> f64 {
        Self::clamp(scale * Self::STEP_FACTOR.powf(notches), min_scale)
    }

    pub fn clamp(scale: f64, min_scale: f64) -> f64 {
        scale.clamp(min_scale.min(Self::MAX_SCALE), Self::MAX_SCALE)
    }

    /// The offset that keeps the image point under the cursor where it is when the scale
    /// changes. `anchor` is in the image's own, untransformed coordinates.
    pub fn zoom_offset(offset: f64, anchor: f64, old_scale: f64, new_scale: f64) -> f64 {
        offset + anchor * (old_scale - new_scale)
    }

    /// Keeps the picture from being dragged out of view. Along an axis where the scaled image is
    /// smaller than the pane it is centred; where it is larger, no edge may come inside the
    /// pane's. `offset` is the translation being proposed, `origin` where layout put the
    /// unscaled image's leading edge within the pane, `length` the unscaled (fitted) image's
    /// length along this axis, and `viewport` the pane's.
    pub fn clamp_offset(offset: f64, origin: f64, length: f64, scale: f64, viewport: f64) -> f64 {
        let scaled = length * scale;
        if scaled <= viewport {
            return (viewport - scaled) / 2.0 - origin;
        }
        offset.clamp(viewport - scaled - origin, -origin)
    }
}
