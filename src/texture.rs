use std::num::NonZeroU64;

/// A unique identifier for a texture resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextureId(pub NonZeroU64);
