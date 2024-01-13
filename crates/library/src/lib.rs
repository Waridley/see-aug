use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Piece {
	pages: Pages,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Pages {
	Images(Vec<ImageHandle>),
	Svg(SvgHandle),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ImageHandle;

#[derive(Debug, Serialize, Deserialize)]
pub struct SvgHandle;

#[cfg(test)]
mod tests {}
