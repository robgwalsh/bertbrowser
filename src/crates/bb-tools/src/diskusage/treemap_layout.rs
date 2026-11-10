//! Squarified treemap packing: divides a rectangle into areas proportional to a set of weights,
//! keeping each piece as close to square as it can.
//!
//! The naive alternative, slice-and-dice (laying every item across in one direction), produces
//! areas that are just as proportional and completely unreadable: with any spread of sizes the
//! small items become hairlines a pixel wide that cannot be labelled, clicked, or compared.
//! Squarifying is what makes the picture worth drawing, so `treemap_layout_tests` holds it to a
//! worst-aspect-ratio bound that a strip layout fails.
//!
//! Pure and UI-free, for the reason the rest of this module is: the geometry is the part worth
//! testing, and it can be tested without a window. Plain `f64` so the C++ side can take it
//! through the FFI unchanged. Spec: Core/Services/DiskUsage/TreemapLayout.cs.

/// A rectangle in layout space, and which input it belongs to. `index` is the position in the
/// list handed to `TreemapLayout::arrange`; the layout sorts internally, so this is how a caller
/// finds its own item again.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TreemapRect {
    pub index: usize,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl TreemapRect {
    pub fn new(index: usize, x: f64, y: f64, width: f64, height: f64) -> TreemapRect {
        TreemapRect {
            index,
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(&self) -> f64 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }

    /// How far from square. 1 is a square; larger is worse, in either direction.
    pub fn aspect_ratio(&self) -> f64 {
        if self.width <= 0.0 || self.height <= 0.0 {
            f64::INFINITY
        } else {
            (self.width / self.height).max(self.height / self.width)
        }
    }
}

/// The rectangle still to be filled.
#[derive(Clone, Copy)]
struct Frame {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl Frame {
    /// Rows are laid along the shorter side, which is what keeps them near-square.
    fn shorter(&self) -> f64 {
        self.width.min(self.height)
    }

    fn horizontal(&self) -> bool {
        self.width >= self.height
    }
}

#[derive(Clone, Copy)]
struct Item {
    index: usize,
    weight: f64,
}

pub struct TreemapLayout;

impl TreemapLayout {
    /// Packs `weights` into a `width` × `height` rectangle at the origin, largest first.
    ///
    /// One rectangle per positive weight, in descending weight order. Weights that are zero,
    /// negative or not finite get no rectangle at all: an item with no measured size has no area
    /// to claim, and drawing it one would assert a share nobody established.
    pub fn arrange(weights: &[f64], width: f64, height: f64) -> Vec<TreemapRect> {
        if weights.is_empty() || !is_usable(width) || !is_usable(height) {
            return Vec::new();
        }

        let mut items: Vec<Item> = weights
            .iter()
            .enumerate()
            .filter(|(_, w)| w.is_finite() && **w > 0.0)
            .map(|(index, &weight)| Item { index, weight })
            .collect();
        if items.is_empty() {
            return Vec::new();
        }

        // Descending by weight; a stable sort keeps equal weights in input order, as the C#
        // comparison-sort left them.
        items.sort_by(|a, b| b.weight.total_cmp(&a.weight));

        let total: f64 = items.iter().map(|i| i.weight).sum();
        let scale = width * height / total;

        let mut result = Vec::with_capacity(items.len());
        squarify(
            &items,
            scale,
            Frame {
                x: 0.0,
                y: 0.0,
                width,
                height,
            },
            &mut result,
        );
        result
    }
}

fn is_usable(value: f64) -> bool {
    value.is_finite() && value > 0.0
}

/// Iterative rather than recursive: a folder can hold tens of thousands of children, and this
/// would otherwise recurse once per row.
fn squarify(items: &[Item], scale: f64, mut frame: Frame, output: &mut Vec<TreemapRect>) {
    let mut next = 0;
    while next < items.len() && is_usable(frame.width) && is_usable(frame.height) {
        // Grow a row one item at a time for as long as doing so makes its worst rectangle
        // squarer. The moment adding another would make it worse, the row is finished.
        let row_start = next;
        let mut row_area = 0.0;
        let mut worst = f64::INFINITY;

        while next < items.len() {
            let area = items[next].weight * scale;
            let row = &items[row_start..next];
            let candidate =
                worst_aspect(row_area + area, area, min_area(row, area), frame.shorter());

            if !row.is_empty() && candidate > worst {
                break;
            }

            row_area += area;
            worst = candidate;
            next += 1;
        }

        frame = place_row(&items[row_start..next], row_area, frame, output);
    }
}

fn min_area(row: &[Item], candidate_area: f64) -> f64 {
    match row.last() {
        None => candidate_area,
        Some(last) => candidate_area.min(last.weight),
    }
}

/// The worst aspect ratio a row would have. The row's thickness is its area divided by the side
/// it runs along, so the extremes come from its largest and smallest members.
fn worst_aspect(row_area: f64, largest: f64, smallest: f64, side: f64) -> f64 {
    if row_area <= 0.0 || side <= 0.0 {
        return f64::INFINITY;
    }

    let thickness = row_area / side;
    if thickness <= 0.0 {
        return f64::INFINITY;
    }

    let widest = largest / thickness;
    let narrowest = smallest / thickness;
    if widest <= 0.0 || narrowest <= 0.0 {
        return f64::INFINITY;
    }

    (thickness / widest)
        .max(widest / thickness)
        .max((thickness / narrowest).max(narrowest / thickness))
}

/// Lays one finished row along the frame's shorter side and returns what is left.
fn place_row(row: &[Item], row_area: f64, frame: Frame, output: &mut Vec<TreemapRect>) -> Frame {
    if row.is_empty() {
        return Frame {
            width: 0.0,
            height: 0.0,
            ..frame
        };
    }

    let side = frame.shorter();
    let along = if frame.horizontal() {
        frame.width
    } else {
        frame.height
    };
    let thickness = (row_area / side).min(along);

    let mut offset = 0.0;
    let row_total: f64 = row.iter().map(|r| r.weight).sum();

    for (i, item) in row.iter().enumerate() {
        // The last piece takes whatever is left, so rounding never leaves a seam.
        let length = if i == row.len() - 1 {
            side - offset
        } else {
            side * (item.weight / row_total)
        };

        output.push(if frame.horizontal() {
            TreemapRect::new(item.index, frame.x, frame.y + offset, thickness, length)
        } else {
            TreemapRect::new(item.index, frame.x + offset, frame.y, length, thickness)
        });

        offset += length;
    }

    if frame.horizontal() {
        Frame {
            x: frame.x + thickness,
            width: frame.width - thickness,
            ..frame
        }
    } else {
        Frame {
            y: frame.y + thickness,
            height: frame.height - thickness,
            ..frame
        }
    }
}
