#![feature(async_closure)]
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
		let reader = async_mrs::tokio::read::fs::ZipFileReader::new(&path)
			.await
			.map_err(|e| std::io::Error::other(e))?;
		let mrs_filename = path.display();
		
		let mut buf = Vec::new();
		let mut backbuf = Vec::new();
		
		let mut task = {
			let reader = reader.clone();
			(reader.file().entries().len() > 0).then(|| tokio::spawn(async move {
				reader
					.reader_with_entry(0).await
					.map_err(ZipErr)?
					.read_to_end_checked(&mut backbuf)
					.await
					.map_err(ZipErr)?;
				Result::<_, MRSError>::Ok(backbuf)
			}))
		};
		
		for i in 0..reader.file().entries().len() {
			let entry_path = PathBuf::from(
				String::from_utf8_lossy(reader.file().entries()[i].filename().as_bytes()).as_ref(),
			);
			let entry_filename = entry_path.display();
			
			let mut backbuf = match task.take().unwrap().await.unwrap() {
				Ok(backbuf) => backbuf,
				Err(e) => {
					error!("couldn't load entry {} (`{entry_filename}`) from `{mrs_filename}`: {e}", i);
					continue;
				},
			};
			std::mem::swap(&mut buf, &mut backbuf);
			let reader = reader.clone();
			task = (reader.file().entries().len() > i + 1).then(|| tokio::spawn(async move {
				backbuf.clear();
				reader
					.reader_with_entry(i + 1).await
					.map_err(ZipErr)?
					.read_to_end_checked(&mut backbuf)
					.await
					.map_err(ZipErr)?;
				Ok(backbuf)
			}));
			
			
			let Some(stem) = entry_path.file_stem().and_then(OsStr::to_str) else {
				error!("archived file `{entry_filename}` is missing file stem (is a sub-directory?)");
				continue;
			};

			match entry_path.extension().and_then(OsStr::to_str) {
				Some("png") => {
					let Some((stem, page_num)) = stem.rsplit_once("-") else {
						error!("unexpected archived file name `{entry_filename}`");
						continue;
					};
					let page_num = page_num.parse::<usize>().map_err(std::io::Error::other)?;
					pages.resize_with(usize::max(page_num, pages.len()), PageImages::default);
					let i = page_num - 1;
					// let img = image::load_from_memory(&*buf).map_err(ImageErr);
					let img = Ok(PageImage::Png(buf.clone()));
					match stem {
						"page" => pages[i].page = img,
						"thumbnail" => pages[i].thumbnail = img,
						"annotations-local" => pages[i].annotations_local = img,
						"annotations-remote" => pages[i].annotations_remote = img,
						other => error!("unexpected file stem `{other}` in archived file `{entry_filename}`"),
					}
				}
				Some("xml") => {
					let buf = std::str::from_utf8(&buf)
						.map_err(|e| XmlErr(quick_xml::de::DeError::Custom(e.to_string())));
					match stem {
						"info" => {
							info = buf.and_then(|s| quick_xml::de::from_str(s).map_err(XmlErr))
						}
						"bookmarks" => {
							bookmarks =
								buf.and_then(|s| quick_xml::de::from_str(s).map_err(XmlErr))
						}
						other => error!("unexpected xml file: `{other}`"),
					}
				}
				Some(other) => error!("unknown extension `{other}`"),
				None => error!("`{entry_filename}` is missing extension"),
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

type ImageResult = Result<PageImage, MRSError>;

#[derive(Default, Debug)]
pub enum MRSError {
	#[default]
	Missing,
	ZipErr(async_mrs::error::ZipError),
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

pub trait MRSResultExt {
	type OkTy;
	fn unwrap_if_present(self) -> Option<Self::OkTy>;
	fn expect_if_present(self, msg: impl AsRef<str>) -> Option<Self::OkTy>;
}

impl<T> MRSResultExt for Result<T, MRSError> {
	type OkTy = T;
	fn unwrap_if_present(self) -> Option<Self::OkTy> {
		match self {
			Err(Missing) => None,
			other => Some(other.unwrap()),
		}
	}
	fn expect_if_present(self, msg: impl AsRef<str>) -> Option<Self::OkTy> {
		match self {
			Err(Missing) => None,
			other => Some(other.expect(msg.as_ref())),
		}
	}
}

impl<'a, T> MRSResultExt for &'a Result<T, MRSError> {
	type OkTy = &'a T;
	
	fn unwrap_if_present(self) -> Option<Self::OkTy> {
		match self {
			Err(Missing) => None,
			other => Some(other.as_ref().unwrap()),
		}
	}
	fn expect_if_present(self, msg: impl AsRef<str>) -> Option<Self::OkTy> {
		match self {
			Err(Missing) => None,
			other => Some(other.as_ref().expect(msg.as_ref())),
		}
	}
}

#[derive(Debug)]
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

impl std::fmt::Debug for PageImage {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		use DynamicImage::*;
		f.write_str(match self {
			Self::Png(_) => "Png(..)",
			Self::DynImg(ImageLuma8(_)) => "Luma8(..)",
			Self::DynImg(ImageLumaA8(_)) => "LumaA8(..)",
			Self::DynImg(ImageRgb8(_)) => "Rgb8(..)",
			Self::DynImg(ImageRgba8(_)) => "Rgba8(..)",
			Self::DynImg(ImageLuma16(_)) => "Luma16(..)",
			Self::DynImg(ImageLumaA16(_)) => "LumaA16(..)",
			Self::DynImg(ImageRgb16(_)) => "Rgb16(..)",
			Self::DynImg(ImageRgba16(_)) => "Rgba16(..)",
			Self::DynImg(ImageRgb32F(_)) => "Rgb32F(..)",
			Self::DynImg(ImageRgba32F(_)) => "Rgba32F(..)",
			Self::DynImg(_) => "DynamicImage",
		})
	}
}

impl Default for PageImages {
	fn default() -> Self {
		Self::new()
	}
}

pub enum PageImage {
	Png(Vec<u8>),
	DynImg(DynamicImage),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Piece {
	pub information: Information,
	pub pages: Pages,
	pub measures: Option<Measures>,
	pub parts: Option<Parts>,
	pub recordings: Option<Recordings>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Information {
	pub identifier: String,
	pub title: String,
	pub creator: Option<Vec<String>>,
	pub description: Option<String>,
	pub subject: Option<Vec<String>>,
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
	pub measure: Option<Vec<Measure>>,
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
	pub part: Option<Vec<Part>>,
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
	pub bookmark: Option<Vec<Bookmark>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Bookmark {
	pub r#type: String,
	pub pageimage: usize,
	pub location: BookmarkLocation,
	pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BookmarkLocation {
	pub x: usize,
	pub y: usize,
}

#[cfg(test)]
mod tests {
	use test_log::test;
	use log::warn;
	use tokio::task::JoinSet;
	use super::*;

	#[test(tokio::test)]
	async fn load_files() {
		MRSFile::load("Bourree_annotated.mrs").await.unwrap();
		MRSFile::load("Menuet.mrs").await.unwrap();
	}
	
	#[ignore]
	#[test(tokio::test)]
	async fn load_dir() {
		let glob = glob::glob(&*(std::env::var("MR_LIB_DIR").unwrap() + "*.mrs")).expect("please set MR_LIB_DIR environment variable to run this test");
		let mut set = JoinSet::new();
		for path in glob {
			let path = match path {
				Ok(path) => path,
				Err(e) => { warn!("{e}"); continue },
			};
			
			set.spawn(async move {
				match MRSFile::load(&path).await {
					Ok(file) => Some(file),
					Err(e) => {
						error!("{e} ({})", path.display());
						return None
					}
				}
			});
		}
		let mut failed = 0;
		while let Some(file) = set.join_next().await {
			if file.unwrap().is_none() { failed += 1 }
		}
		if failed > 0 { panic!("{failed} files failed to load")}
	}
	
	macro_rules! expect_fields {
    ($path:expr => [$($var:ident),*$(,)?]) => {
	    $(
	      #[allow(unused)]
	      let $var = $var.expect(&*format!("{} -> {}", $path.display(), stringify!($var)));
	    )*
    };
	}
	
	#[test(tokio::test)]
	async fn parse_files() {
		let MRSFile {
			path, pages, bookmarks, info
		} = MRSFile::load("Bourree_annotated.mrs").await.unwrap();
		
		assert_eq!(pages.len(), 2);
		for (i, PageImages { page, thumbnail, annotations_local, annotations_remote }) in pages.into_iter().enumerate() {
			let path = path.join(format!("page-{}", i + 1));
			expect_fields!(path => [page, thumbnail, annotations_local, annotations_remote]);
		}
		
		expect_fields!(path => [bookmarks]);
		let bookmark = bookmarks.bookmark;
		expect_fields!(path => [bookmark]);
		assert_eq!(bookmark.len(), 1);
		
		let Piece {
			information: Information {
				identifier, title, creator, description, subject, publisher, copyright
			},
			pages, measures, parts, recordings,
		} = info.expect(&format!("{} -> info.xml", path.display()));
		
		expect_fields!(path => [creator, description, publisher, copyright, measures, parts, recordings]);
		
		assert_eq!(identifier, "MR76928326");
		assert_eq!(title, "English Suite I: Bourree I");
		assert_eq!(creator, vec!["Bach, Johann Sebastian [composer]"]);
		assert_eq!(subject.as_ref().map(Vec::len), Some(3));
		assert_eq!(pages.page.len(), 2);
	}
	
	
	#[ignore]
	#[test(tokio::test)]
	async fn parse_dir() {
		let glob = glob::glob(&*(std::env::var("MR_LIB_DIR").unwrap() + "*.mrs")).expect("please set MR_LIB_DIR environment variable to run this test");
		
		macro_rules! expect_if_present {
	    ($path:expr => [$($var:ident),*$(,)?]) => {
		    $(
		      #[allow(unused)]
		      let $var = match $var {
						Err(Missing) => None,
						Ok(val) => Some(val),
			      Err(e) => {
							error!("{e} ({} -> {})", $path.display(), stringify!($var));
							return None
						}
					};
		    )*
	    };
		}
		
		let (tx, mut rx) = tokio::sync::mpsc::channel(32);
		
		tokio::spawn(async move {
			for path in glob {
				let path = match path {
					Ok(path) => path,
					Err(e) => { warn!("{e}"); continue },
				};
				
				tx.send(tokio::spawn(async move {
					let MRSFile { path, pages, bookmarks, info } = match MRSFile::load(&path).await {
						Ok(file) => file,
						Err(e) => {
							warn!("{e} ({})", path.display());
							// Files that fail to load will be caught by `load_dir`. Just check if the rest parse here.
							return Some(())
						}
					};
					
					for (i, PageImages { page, thumbnail, annotations_local, annotations_remote }) in pages.into_iter().enumerate() {
						let path = path.join(format!("<page {}>", i + 1));
						expect_if_present!(path => [page, annotations_local, annotations_remote]);
						if let Err(e) = thumbnail {
							// can always regenerate thumbnails
							warn!("{e} ({} -> thumbnail)", path.display());
						}
					}
					
					expect_if_present!(path => [bookmarks]);
					
					match info {
						Ok(_) => {}
						Err(e) => {
							error!("{e} ({} -> info.xml)", path.display());
							return None
						}
					};
					
					Some(())
				})).await.unwrap();
			}
		});
		let mut failed = 0;
		while let Some(jh) = rx.recv().await {
			let file = jh.await;
			if file.unwrap().is_none() { failed += 1 }
		}
		if failed > 0 { panic!("{failed} files failed to parse")}
	}
}
