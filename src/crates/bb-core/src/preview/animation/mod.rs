//! Animated GIFs in the preview: the size gate, the frame clock, the compositor that turns
//! sub-frames into whole pictures, and the decoder that reads them out of the file.
//! Spec: Core/Services/Preview/Animation/*.

mod gif;
mod gif_decode;
mod limits;
mod timeline;

pub use gif::{GifDisposal, GifFrameCompositor, GifSubFrame};
pub use gif_decode::{GifAnimation, GifDecoder, GifError, GifHeader};
pub use limits::AnimationLimits;
pub use timeline::AnimationTimeline;
