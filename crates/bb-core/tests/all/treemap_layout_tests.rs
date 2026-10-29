// Port of tests/BertBrowser.Core.Tests/TreemapLayoutTests.cs
//
// The treemap geometry. Three properties make a treemap a treemap — the pieces fill the box, they
// do not overlap, and their areas are proportional — and a fourth makes it readable, which is that
// they are near-square rather than hairlines.
use bb_core::diskusage::{TreemapLayout, TreemapRect};

const EPSILON: f64 = 1e-6;

/// xUnit's `Assert.Equal(expected, actual, precision)` rounds both to `precision` decimals.
fn assert_close(expected: f64, actual: f64, precision: i32, context: &str) {
    let scale = 10f64.powi(precision);
    assert_eq!(
        (expected * scale).round(),
        (actual * scale).round(),
        "{context}: expected {expected}, got {actual}"
    );
}

/// A deterministic stand-in for `new Random(seed).NextDouble()`: the test only needs a repeatable
/// spread of weights, not .NET's exact sequence.
fn pseudo_random(seed: u64) -> impl FnMut() -> f64 {
    let mut state = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    move || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 11) as f64 / (1u64 << 53) as f64
    }
}

// --- The three properties that make it a treemap ---

#[test]
fn every_rectangle_stays_inside_the_bounds() {
    for count in [1, 7, 64] {
        let weights: Vec<f64> = (1..=count).map(|i| (i * i) as f64).collect();

        for rect in TreemapLayout::arrange(&weights, 800.0, 500.0) {
            assert!(
                rect.x >= -EPSILON,
                "count {count}: {rect:?} starts left of the box"
            );
            assert!(
                rect.y >= -EPSILON,
                "count {count}: {rect:?} starts above the box"
            );
            assert!(
                rect.right() <= 800.0 + EPSILON,
                "count {count}: {rect:?} runs past the right edge"
            );
            assert!(
                rect.bottom() <= 500.0 + EPSILON,
                "count {count}: {rect:?} runs past the bottom edge"
            );
        }
    }
}

#[test]
fn rectangles_do_not_overlap() {
    for seed in [1u64, 3, 17, 101] {
        let mut next = pseudo_random(seed);
        let weights: Vec<f64> = (0..40).map(|_| next() * 1000.0 + 1.0).collect();

        let overlaps = find_overlaps(&TreemapLayout::arrange(&weights, 640.0, 400.0));

        assert!(overlaps.is_none(), "seed {seed}: overlap: {overlaps:?}");
    }
}

#[test]
fn areas_are_proportional_to_weights() {
    let weights = [50.0, 25.0, 15.0, 10.0];
    let rects = TreemapLayout::arrange(&weights, 400.0, 300.0);
    let total_area = 400.0 * 300.0;

    for rect in rects {
        let expected = total_area * weights[rect.index] / 100.0;
        assert_close(expected, rect.width * rect.height, 3, &format!("{rect:?}"));
    }
}

#[test]
fn the_whole_box_is_covered() {
    let covered: f64 = TreemapLayout::arrange(&[5.0, 4.0, 3.0, 2.0, 1.0], 300.0, 200.0)
        .iter()
        .map(|r| r.width * r.height)
        .sum();

    assert_close(300.0 * 200.0, covered, 3, "covered area");
}

// --- The property that makes it readable ---

/// The reason for the algorithm. Slice-and-dice would satisfy every test above and still be
/// useless: with a spread of sizes the small items become hairlines that cannot be labelled or
/// clicked. Replace the squarifying with a single strip and this goes red.
#[test]
fn squarified_beats_slice_and_dice() {
    let weights = [600.0, 300.0, 60.0, 20.0, 10.0, 6.0, 3.0, 1.0];

    let squarified = worst_aspect(&TreemapLayout::arrange(&weights, 600.0, 400.0));
    let sliced = worst_aspect(&slice_and_dice(&weights, 600.0, 400.0));

    assert!(squarified < 6.0, "worst aspect ratio was {squarified:.1}");
    assert!(
        squarified < sliced,
        "squarified {squarified:.1} was no better than sliced {sliced:.1}"
    );
}

// --- Degenerate input returns nothing rather than throwing ---

#[test]
fn an_unusable_box_yields_nothing() {
    for (width, height) in [
        (0.0, 100.0),
        (100.0, 0.0),
        (-5.0, 100.0),
        (f64::NAN, 100.0),
        (f64::INFINITY, 100.0),
    ] {
        assert!(
            TreemapLayout::arrange(&[1.0, 2.0, 3.0], width, height).is_empty(),
            "box {width} x {height}"
        );
    }
}

#[test]
fn no_weights_yield_nothing() {
    assert!(TreemapLayout::arrange(&[], 100.0, 100.0).is_empty());
}

/// An item with no measured size has no area to claim, and giving it one would assert a share
/// nobody established — the same rule the rest of this feature follows about unknown sizes.
#[test]
fn zero_and_negative_and_non_finite_weights_get_no_rectangle() {
    let rects = TreemapLayout::arrange(
        &[10.0, 0.0, -5.0, f64::NAN, f64::INFINITY, 5.0],
        200.0,
        100.0,
    );

    let mut indices: Vec<usize> = rects.iter().map(|r| r.index).collect();
    indices.sort_unstable();
    assert_eq!(vec![0, 5], indices);
}

#[test]
fn a_single_item_fills_the_whole_box() {
    let rects = TreemapLayout::arrange(&[42.0], 120.0, 80.0);
    assert_eq!(1, rects.len());
    let rect = rects[0];

    assert_eq!(0, rect.index);
    assert_close(120.0, rect.width, 6, "width");
    assert_close(80.0, rect.height, 6, "height");
}

/// The layout sorts internally, so a caller has to be able to find its own item.
#[test]
fn indices_map_back_to_the_original_order() {
    let rects = TreemapLayout::arrange(&[1.0, 100.0, 10.0], 300.0, 200.0);

    // Largest first, by original position: 100 is at index 1, then 10 at 2, then 1 at 0.
    let indices: Vec<usize> = rects.iter().map(|r| r.index).collect();
    assert_eq!(vec![1, 2, 0], indices);
}

#[test]
fn equal_weights_get_equal_areas() {
    let rects = TreemapLayout::arrange(&[1.0, 1.0, 1.0, 1.0], 200.0, 200.0);

    for rect in rects {
        assert_close(10_000.0, rect.width * rect.height, 3, &format!("{rect:?}"));
    }
}

// --- Meta-test: the overlap check can actually fail ---

/// A check that cannot fail proves nothing, so this hands the helper two rectangles that really
/// do overlap and asserts it says so.
#[test]
fn the_overlap_check_can_fail() {
    let overlapping = [
        TreemapRect::new(0, 0.0, 0.0, 10.0, 10.0),
        TreemapRect::new(1, 5.0, 5.0, 10.0, 10.0),
    ];

    assert!(find_overlaps(&overlapping).is_some());
}

fn worst_aspect(rects: &[TreemapRect]) -> f64 {
    rects
        .iter()
        .map(|r| r.aspect_ratio())
        .fold(f64::NEG_INFINITY, f64::max)
}

fn find_overlaps(rects: &[TreemapRect]) -> Option<String> {
    for i in 0..rects.len() {
        for j in i + 1..rects.len() {
            let a = rects[i];
            let b = rects[j];

            let overlap_width = a.right().min(b.right()) - a.x.max(b.x);
            let overlap_height = a.bottom().min(b.bottom()) - a.y.max(b.y);

            if overlap_width > EPSILON && overlap_height > EPSILON {
                return Some(format!("{a:?} and {b:?}"));
            }
        }
    }
    None
}

/// The naive layout this algorithm exists to beat: one strip, every item across it.
fn slice_and_dice(weights: &[f64], width: f64, height: f64) -> Vec<TreemapRect> {
    let total: f64 = weights.iter().sum();
    let mut rects = Vec::new();
    let mut x = 0.0;

    for (i, weight) in weights.iter().enumerate() {
        let w = width * weight / total;
        rects.push(TreemapRect::new(i, x, 0.0, w, height));
        x += w;
    }
    rects
}
