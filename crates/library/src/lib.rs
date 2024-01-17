use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use image::DynamicImage;
use serde::{Deserialize, Serialize};

pub type SkiaImage = skia_safe::Image;

#[derive(Debug)]
pub struct Piece {
	pub meta: Meta,
	pub pages: Pages,
}

#[derive(Debug)]
pub enum Pages {
	Images(Vec<ImageState>),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Meta {
	pub format: PageFormat,
	pub annotations: AnnotationFormat,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum PageFormat {
	Images {
		files: Vec<PathBuf>,
	},
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Annotations<'meta> {
	pub files: HashMap<Cow<'meta, Path>, AnnotationFormat>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum AnnotationFormat {
	Image,
	Svg,
	Strokes,
}

#[derive(Debug)]
pub enum ImageState {
	Path(PathBuf),
	Encoded(Vec<u8>),
	Decoded(DynamicImage),
	Rendered(SkiaImage),
}

pub enum ImageRef<'a> {
	Path(&'a Path),
	Encoded(&'a [u8]),
	Decoded(&'a DynamicImage),
	Rendered(SkiaImage),
}

#[cfg(test)]
mod tests {}
