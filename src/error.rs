use std::path::PathBuf;

pub type HdrResult<T> = Result<T, HdrError>;

#[derive(Debug)]
pub enum HdrError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    PngDecode {
        path: PathBuf,
        message: String,
    },
    PngEncode(String),
    Job(String),
    Json {
        path: PathBuf,
        message: String,
    },
    Rank {
        channel: char,
        rank: usize,
        needed: usize,
        min_singular: f64,
        max_singular: f64,
    },
    NonFinite {
        channel: char,
        x: usize,
        y: usize,
        value: f64,
    },
    NoValidExposure {
        channel: char,
        x: usize,
        y: usize,
    },
}

impl std::fmt::Display for HdrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HdrError::Io { path, source } => {
                write!(f, "I/O error on {}: {}", path.display(), source)
            }
            HdrError::PngDecode { path, message } => {
                write!(f, "failed to decode PNG {}: {}", path.display(), message)
            }
            HdrError::PngEncode(message) => write!(f, "PNG encode failed: {message}"),
            HdrError::Job(message) => write!(f, "invalid job: {message}"),
            HdrError::Json { path, message } => {
                write!(f, "invalid JSON {}: {}", path.display(), message)
            }
            HdrError::Rank {
                channel,
                rank,
                needed,
                min_singular,
                max_singular,
            } => write!(
                f,
                "channel {channel} response system is rank deficient (rank {rank}/{needed}, sigma_min={min_singular:.6e}, sigma_max={max_singular:.6e}); material is not identifiable"
            ),
            HdrError::NonFinite {
                channel,
                x,
                y,
                value,
            } => write!(
                f,
                "non-finite radiance at ({x},{y}) channel {channel}: {value}"
            ),
            HdrError::NoValidExposure { channel, x, y } => write!(
                f,
                "no usable exposure at ({x},{y}) channel {channel}"
            ),
        }
    }
}

impl std::error::Error for HdrError {}
