use image::DynamicImage;
use log::error;
use serde::{Deserialize, Serialize};
use std::{
	ffi::OsStr,
	fmt::{Display, Formatter},
	path::{Path, PathBuf},
};
use MRSError::*;

/// A loaded MusicReader (`.mrs`) file
#[derive(Debug)]
pub struct MRSFile {
	pub path: PathBuf,
	pub pages: Vec<PageImages>,
	pub bookmarks: Result<Bookmarks, MRSError>,
	pub info: Result<Piece, MRSError>,
}

impl MRSFile {
	pub async fn load(path: impl AsRef<Path>) -> std::io::Result<Self> {
		let mut pages = Vec::new();
		let mut bookmarks = Err(Missing);
		let mut info = Err(Missing);

		let path = PathBuf::from(path.as_ref());
		let reader = async_zip::tokio::read::fs::ZipFileReader::new(&path)
			.await
			.map_err(|e| std::io::Error::other(e))?;

		for i in 0..reader.file().entries().len() {
			let mut reader = match reader.reader_with_entry(i).await {
				Ok(file) => file,
				Err(e) => {
					error!("failed to load archived file: {e}");
					continue;
				}
			};

			let path = PathBuf::from(
				String::from_utf8_lossy(reader.entry().filename().as_bytes()).as_ref(),
			);
			let name = path.display();

			let Some(stem) = path.file_stem().and_then(OsStr::to_str) else {
				error!("archived file `{name}` is missing file stem (is a sub-directory?)");
				continue;
			};

			let mut buf = Vec::new();
			let buf = reader
				.read_to_end_checked(&mut buf)
				.await
				.map_err(ZipErr)
				.map(|_| buf);

			match path.extension().and_then(OsStr::to_str) {
				Some("png") => {
					let Some((stem, page_num)) = stem.rsplit_once("-") else {
						error!("unexpected archived file name `{name}`");
						continue;
					};
					let page_num = page_num.parse::<usize>().map_err(std::io::Error::other)?;
					pages.resize_with(usize::max(page_num, pages.len()), PageImages::default);
					let i = page_num - 1;
					let img = buf.and_then(|buf| image::load_from_memory(&*buf).map_err(ImageErr));
					match stem {
						"page" => pages[i].page = img,
						"thumbnail" => pages[i].thumbnail = img,
						"annotations-local" => pages[i].annotations_local = img,
						"annotations-remote" => pages[i].annotations_remote = img,
						other => error!("unexpected file stem `{other}` in archived file `{name}`"),
					}
				}
				Some("xml") => {
					let buf = buf.and_then(|buf| {
						String::from_utf8(buf)
							.map_err(|e| XmlErr(quick_xml::de::DeError::Custom(e.to_string())))
					});
					match stem {
						"info" => {
							info = buf.and_then(|s| quick_xml::de::from_str(&s).map_err(XmlErr))
						}
						"bookmarks" => {
							bookmarks =
								buf.and_then(|s| quick_xml::de::from_str(&s).map_err(XmlErr))
						}
						other => error!("unexpected xml file: `{other}`"),
					}
				}
				Some(other) => error!("unknown extension `{other}`"),
				None => error!("`{name}` is missing extension"),
			}
		}

		Ok(Self {
			path,
			pages,
			bookmarks,
			info,
		})
	}
}

pub struct PageImages {
	pub page: ImageResult,
	pub thumbnail: ImageResult,
	pub annotations_local: ImageResult,
	pub annotations_remote: ImageResult,
}

impl PageImages {
	pub fn new() -> Self {
		Self {
			page: Err(Missing),
			thumbnail: Err(Missing),
			annotations_local: Err(Missing),
			annotations_remote: Err(Missing),
		}
	}
}

impl std::fmt::Debug for PageImages {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		fn variant_only(img: &DynamicImage) -> impl std::fmt::Debug {
			struct VariantName(&'static str);
			impl std::fmt::Debug for VariantName {
				fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
					write!(f, "{}", self.0)
				}
			}

			use DynamicImage::*;
			VariantName(match img {
				ImageLuma8(_) => "Luma8(..)",
				ImageLumaA8(_) => "LumaA8(..)",
				ImageRgb8(_) => "Rgb8(..)",
				ImageRgba8(_) => "Rgba8(..)",
				ImageLuma16(_) => "Luma16(..)",
				ImageLumaA16(_) => "LumaA16(..)",
				ImageRgb16(_) => "Rgb16(..)",
				ImageRgba16(_) => "Rgba16(..)",
				ImageRgb32F(_) => "Rgb32F(..)",
				ImageRgba32F(_) => "Rgba32F(..)",
				_ => "DynamicImage",
			})
		}

		f.debug_struct("Page")
			.field("page", &self.page.as_ref().map(variant_only))
			.field("thumbnail", &self.thumbnail.as_ref().map(variant_only))
			.field(
				"annotations_local",
				&self.annotations_local.as_ref().map(variant_only),
			)
			.field(
				"annotations_remote",
				&self.annotations_remote.as_ref().map(variant_only),
			)
			.finish()
	}
}

impl Default for PageImages {
	fn default() -> Self {
		Self::new()
	}
}

type ImageResult = Result<DynamicImage, MRSError>;

#[derive(Default, Debug)]
pub enum MRSError {
	#[default]
	Missing,
	ZipErr(async_zip::error::ZipError),
	ImageErr(image::ImageError),
	XmlErr(quick_xml::DeError),
	IoErr(std::io::Error),
}

impl Display for MRSError {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			Missing => f.write_str("image file not found in archive"),
			ZipErr(e) => e.fmt(f),
			ImageErr(e) => e.fmt(f),
			XmlErr(e) => e.fmt(f),
			IoErr(e) => e.fmt(f),
		}
	}
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Piece {
	pub information: Information,
	pub pages: Pages,
	pub measures: Measures,
	pub parts: Parts,
	pub recordings: Option<Recordings>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Information {
	pub identifier: String,
	pub title: String,
	pub creator: Option<String>,
	pub description: Option<String>,
	pub subject: Vec<String>,
	pub publisher: Option<String>,
	pub copyright: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Pages {
	pub page: Vec<Page>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Page {
	pub number: usize,
	pub image: usize,
	pub pageturn: PageTurn,
	pub halfpage: Option<f32>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PageTurn {
	Whole,
	Half,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Measures {
	pub measure: Vec<Measure>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Measure {
	pub number: usize,
	pub number_of_measures: usize,
	pub movement: usize,
	pub image: usize,
	pub x_left: usize,
	pub x_right: usize,
	pub y_top: usize,
	pub y_bottom: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Parts {
	pub image: Vec<ImageInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ImageInfo {
	pub part: Vec<Part>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Part {
	pub x: usize,
	pub y: usize,
	pub width: usize,
	pub height: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Recordings {
	// TODO: Figure out what fields Recordings should have
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Bookmarks {
	// TODO: Figure out what fields Bookmarks should have
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	async fn load_files() {
		MRSFile::load("Bourree.mrs").await.unwrap();
	}
}
