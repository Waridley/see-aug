#![allow(non_snake_case)]

use std::ops::Deref;
use std::sync::Arc;
use freya::prelude::*;
use image::GenericImageView;
use mr_imp::{MRSFile, PageImage,};
use crate::annotations::AnnotationCanvas;

#[component]
pub fn PieceView<'a>(cx: Scope<'a>, width: &'a str, height: &'a str) -> Element {
	let piece = cx.consume_context::<OpenPiece>();
	
	let image_datas = use_ref(cx, || vec![]);
	
	piece.as_ref().map(|file| {
		if image_datas.with(Vec::len) != file.pages.len() {
			image_datas.write().resize(file.pages.len(), None);
		}
		for (i, images) in file.pages.iter().enumerate() {
			if image_datas.with(|image_datas| image_datas[i].is_none()) {
				image_datas.with_mut(|image_datas| {
					image_datas[i] = Some(images.page.as_ref()
						.map(|img| {
							match img {
								PageImage::Png(img) => img.clone(),
								PageImage::DynImg(img) => img.pixels().flat_map(|(_, _, pixel)| pixel.0).collect::<Vec<_>>(),
							}
						})
						.unwrap());
				})
			}
		}
	});
	
	let mut images = vec![];
	for data in &*image_datas.read() {
		let data = data.as_deref().unwrap();
		let data = bytes_to_data(cx, data);
		images.push(rsx!(
			Page {
				System {
					image {
						width: "100%",
						height: "283",
						image_data: data,
					},
				},
			},
			rect {
				width: "100%",
				height: "3",
				background: "transparent",
			}
		));
	}
	
	render! {
		rect {
			width: *width,
			height: *height,
			background: "rgb(20, 20, 20)",
			ScrollView {
				theme: theme_with!(ScrollViewTheme {
					width: "100%".into(),
					height: "100%".into(),
				}),
				images.into_iter(),
			},
		}
	}
}

#[component]
pub fn Page<'a>(cx: Scope<'a>, children: Element<'a>) -> Element {
	render! {
		AnnotationCanvas {
			rect {
				width: "100%",
				children
			}
		}
	}
}

#[component]
pub fn System<'a>(cx: Scope<'a>, children: Element<'a>) -> Element {
	render! {
		rect {
			width: "100%",
			children
		}
	}
}

#[derive(Clone)]
pub struct OpenPiece(pub Arc<MRSFile>);

impl Deref for OpenPiece {
	type Target = MRSFile;
	
	fn deref(&self) -> &Self::Target {
		&*self.0
	}
}
