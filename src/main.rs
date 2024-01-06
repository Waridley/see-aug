#![cfg_attr(
	all(not(debug_assertions), target_os = "windows"),
	windows_subsystem = "windows"
)]

use dioxus::core::AttributeValue;
use freya::elements::{onmousedown, ontouchstart};
use freya::events::touch::TouchPhase;
use freya::prelude::*;
use image::DynamicImage::ImageRgba8;
use image::{ImageOutputFormat, Pixel, Rgba, RgbaImage};
use pointer::{MouseButton, PointerType};
use skia_safe::wrapper::NativeTransmutableWrapper;
use skia_safe::{AlphaType, ColorType, Data, ISize, ImageInfo, Paint, PathBuilder, RCHandle, PathEffect, Matrix};
use std::collections::VecDeque;
use std::io::Cursor;
use std::sync::{Arc, Mutex};

fn main() {
	launch_cfg(
		app,
		LaunchConfig::<()>::builder()
			.with_title("See Augmented")
			.build(),
	);
}

#[derive(Debug, Copy, Clone, PartialEq)]
enum PathMsg {
	Start(CursorPoint, f64),
	Move(CursorPoint, f64),
	End(CursorPoint),
}

fn app(cx: Scope) -> Element {
	let pen_down = use_state(cx, || false);
	
	let channel = use_ref(cx, || crossbeam::channel::unbounded());
	let paths = use_ref(cx, || Arc::new(Mutex::new(Vec::<VecDeque<(CursorPoint, f64)>>::new())));

	let canvas = use_canvas(cx, channel, |channel| {
		let rx = channel.read().1.clone();
		let paths = paths.read().clone();
		Box::new(move |canvas, fonts, area| {
			let mut paths = paths.lock().unwrap();
			for msg in rx.try_iter() {
				match msg {
					PathMsg::Start(pos, force) => paths.push(VecDeque::from([(pos, force)])),
					PathMsg::Move(pos, force) => {
						let Some(mut path) = paths.last_mut() else {
							eprintln!("ERROR: trying to continue path that is not started");
							continue;
						};
						path.push_back((pos, force));
					}
					PathMsg::End(pos) => {
						let Some(mut path) = paths.last_mut() else {
							eprintln!("ERROR: trying to continue path that is not started");
							continue;
						};
						path.push_back((pos, 0.0));
					}
				}
			}
			let mut paint = Paint::default();
			paint.set_stroke_width(4.0);
			for mut points in &*paths {
				let mut points = points.iter().copied();
				let mut last = points.next().unwrap();
				paint.set_alpha((255.0 * last.1) as u8);
				for (point, force) in points {
					paint.set_alpha((255.0 * force) as u8);
					let start = skia_safe::Point::new(last.0.x as f32, last.0.y as f32);
					let end = skia_safe::Point::new(point.x as f32, point.y as f32);

					canvas.draw_line(start, end, &paint);
					last = (point, force);
				}
			}
		})
	});
	
	let on_touch = |e: TouchEvent| {
		let TouchData {
			element_coordinates: pos,
			finger_id,
			force,
			phase,
			..
		} = **e;
		let force = if let Some(force) = force {
			force.normalized()
		} else {
			1.0
		};
		let msg = match phase {
			TouchPhase::Started => PathMsg::Start(pos, force),
			TouchPhase::Moved => PathMsg::Move(pos, force),
			TouchPhase::Ended => PathMsg::End(pos),
			TouchPhase::Cancelled => PathMsg::End(pos), // TODO: Can we cancel strokes?
		};
		channel.write().0.send(msg).unwrap();
	};

	let start_path = |e: MouseEvent| {
		if matches!(e.trigger_button, Some(MouseButton::Left)) {
			pen_down.set(true);
			channel
				.write()
				.0
				.send(PathMsg::Start(e.element_coordinates, 1.0))
				.unwrap();
		}
	};
	let continue_path = |e: MouseEvent| {
		if *pen_down.get() {
			channel
				.write()
				.0
				.send(PathMsg::Move(e.element_coordinates, 1.0))
				.unwrap();
		}
	};
	let end_path = |e: MouseEvent| {
		if matches!(e.trigger_button, Some(MouseButton::Left)) {
			pen_down.set(false);
			channel
				.write()
				.0
				.send(PathMsg::End(e.element_coordinates))
				.unwrap();
		}
	};

	render!(
		rect {
			width: "100%",
			height: "100%",
			display: "center",
			ontouchstart: on_touch,
			ontouchmove: on_touch,
			ontouchend: on_touch,
			onmousedown: start_path,
			onmouseover: continue_path,
			onclick: end_path,
			Canvas {
				canvas: canvas,
				width: "100%",
				height: "100%",
			}
		}
	)
}
