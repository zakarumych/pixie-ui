use std::fmt;

/// Error type returned by [`crate::Renderer`] operations.
#[derive(Debug)]
pub enum Error {
    /// Failed to compile/parse a shader library.
    Shader(mev::ShaderLibraryError),

    /// Failed to create a render pipeline.
    Pipeline(mev::PipelineError),

    /// A device-level error occurred (out of memory / device lost).
    Device(mev::DeviceError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Shader(err) => write!(f, "shader error: {err}"),
            Error::Pipeline(err) => write!(f, "pipeline error: {err}"),
            Error::Device(err) => write!(f, "device error: {err}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<mev::ShaderLibraryError> for Error {
    fn from(err: mev::ShaderLibraryError) -> Self {
        Error::Shader(err)
    }
}

impl From<mev::PipelineError> for Error {
    fn from(err: mev::PipelineError) -> Self {
        Error::Pipeline(err)
    }
}

impl From<mev::DeviceError> for Error {
    fn from(err: mev::DeviceError) -> Self {
        Error::Device(err)
    }
}
