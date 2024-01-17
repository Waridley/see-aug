#![cfg_attr(
	all(not(debug_assertions), target_os = "windows"),
	windows_subsystem = "windows"
)]

use std::sync::Arc;
use freya::prelude::*;
use tokio::sync::oneshot::error::TryRecvError;
use mr_imp::MRSFile;
use crate::page_rendering::{OpenPiece, PieceView};

mod annotations;
mod page_rendering;

fn main() {
	launch_cfg(
		app,
		LaunchConfig::<()>::builder()
			.with_title("See Augmented")
			.build(),
	);
}

fn app(cx: Scope) -> Element {
	let rx = use_ref(cx, || {
		let (tx, rx) = tokio::sync::oneshot::channel();
		cx.spawn(async {
			let file = MRSFile::load("crates/mr-imp/Menuet.mrs").await.unwrap();
			tx.send(file).unwrap();
		});
		rx
	});
	
	let open_piece = use_ref(cx, || None);
	match rx.write_silent().try_recv() {
		Ok(file) => { open_piece.set(Some(OpenPiece(Arc::new(file)))); },
		Err(TryRecvError::Closed) => { if open_piece.read().is_none() { panic!("failed to open file") }}
		Err(TryRecvError::Empty) => cx.needs_update(),
	}
	if let Some(piece) = open_piece.read().as_ref() {
		cx.provide_root_context(piece.clone());
	}
	
	render! {
		rect {
			PieceView {
				width: "100%",
				height: "100%",
			}
		}
	}
}
