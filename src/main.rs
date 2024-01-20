#![cfg_attr(
	all(not(debug_assertions), target_os = "windows"),
	windows_subsystem = "windows"
)]

use std::sync::Arc;
use freya::prelude::*;
use tokio::sync::oneshot::error::TryRecvError;
use tracing_subscriber::{filter, fmt, reload, prelude::*, Registry};
use winit::platform::x11::WindowBuilderExtX11;
use freya::events::keyboard::Code;
use mr_imp::MRSFile;
use crate::page_rendering::{OpenPiece, PieceView};
use crate::settings::Settings;

mod annotations;
mod page_rendering;
mod settings;

type ReloadHandle = reload::Handle<filter::Filtered<fmt::Layer<Registry>, filter::LevelFilter, Registry>, Registry>;

#[derive(Debug, Clone)]
pub struct State {
	pub log_reload_handle: ReloadHandle,
}

fn main() {
	let filtered_layer = fmt::layer().with_filter(filter::LevelFilter::DEBUG);
	let (filtered_layer, log_reload_handle) = reload::Layer::new(filtered_layer);
	tracing_subscriber::registry()
		.with(filtered_layer)
		.init();
	
	let state = State {
		log_reload_handle,
	};
	
	let window_hook = |window: winit::window::WindowBuilder| {
		window
			.with_inner_size(winit::dpi::LogicalSize::new(1280, 720))
			.with_maximized(true)
	};
	
	launch_cfg(
		app,
		LaunchConfig::builder()
			.with_title("See Augmented")
			.with_window_builder(window_hook)
			.with_state(state)
			.build(),
	);
}

fn app(cx: Scope) -> Element {
	let state = cx.consume_context::<State>().unwrap();
	let settings = use_ref(cx, Settings::default);
	use_effect(cx, settings, |filter| {
		let filter = filter.read().log_level;
		to_owned![state];
		async move { state.log_reload_handle.modify(|layer| *layer.filter_mut() = filter).unwrap_or_else(|e| eprintln!("Failed to update log level: {e}")); }
	});
	
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
	
	let onkey = |e: Event<KeyboardData>| {
		// TODO: Change this to `Code::Backqoute` when event bubbling works.
		// Or better yet, make a settings window.
		if e.code == Code::Escape {
			let level = match settings.read().log_level {
				filter::LevelFilter::WARN => filter::LevelFilter::DEBUG,
				filter::LevelFilter::DEBUG => filter::LevelFilter::TRACE,
				_ => filter::LevelFilter::WARN,
			};
			settings.with_mut(|settings| settings.log_level = level);
		}
	};
	
	render! {
		rect {
			onkeyup: onkey,
			PieceView {
				width: "100%",
				height: "100%",
			}
		}
	}
}
