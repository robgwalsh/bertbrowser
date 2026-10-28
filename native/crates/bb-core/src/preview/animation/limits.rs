//! How much of an animation the preview is willing to hold decoded. Every frame is kept as a full
//! canvas so a seek is a lookup, which makes the cost frames × width × height × 4; past these the
//! still first frame is shown instead, as it always was.
//! Spec: Core/Services/Preview/Animation/AnimationLimits.cs.

pub struct AnimationLimits;

impl AnimationLimits {
    pub const MAX_FRAMES: usize = 2_000;

    pub const MAX_DECODED_BYTES: u64 = 256 << 20; // 256 MB

    /// Whether an animation of this shape may be decoded whole. Asked of the header, before a
    /// single frame's pixels are read.
    pub fn allows(frame_count: usize, width: u32, height: u32) -> bool {
        frame_count > 1
            && frame_count <= Self::MAX_FRAMES
            && width > 0
            && height > 0
            && frame_count as u64 * u64::from(width) * u64::from(height) * 4
                <= Self::MAX_DECODED_BYTES
    }
}
