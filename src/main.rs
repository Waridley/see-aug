#![feature(array_windows)]
#![cfg_attr(
	all(not(debug_assertions), target_os = "windows"),
	windows_subsystem = "windows"
)]

use freya::events::touch::TouchPhase;
use freya::prelude::*;

use pointer::{MouseButton, PointerType};
use skia_safe::{BlendMode, Color, Paint, Path, Point, Vertices, vertices};
use std::collections::VecDeque;

use std::sync::{Arc, Mutex};
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Relaxed;
use log::error;
use skia_safe::vertices::VertexMode;

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

static VARY_WIDTH: AtomicBool = AtomicBool::new(false);
static VARY_ALPHA: AtomicBool = AtomicBool::new(true);

fn app(cx: Scope) -> Element {
	let pen_down = use_state(cx, || false);

	let channel = use_ref(cx, || crossbeam::channel::unbounded());
	let paths = use_ref(cx, || {
		Arc::new(Mutex::new(Vec::<Vec<(CursorPoint, f64)>>::new()))
	});

	let canvas = use_canvas(cx, channel, |channel| {
		let rx = channel.read().1.clone();
		let paths = paths.read().clone();
		Box::new(move |canvas, _fonts, _area| {
			let mut paths = paths.lock().unwrap();
			for msg in rx.try_iter() {
				match msg {
					PathMsg::Start(pos, force) => paths.push(vec![(pos, force)]),
					PathMsg::Move(pos, force) => {
						let Some(path) = paths.last_mut() else {
							error!("trying to continue path that is not started");
							continue;
						};
						let dist = pos - path.last().unwrap().0;
						if dist.length() > 3.0 { // don't draw lines too short
							path.push((pos, force));
						}
					}
					PathMsg::End(pos) => {
						let Some(path) = paths.last_mut() else {
							error!("trying to continue path that is not started");
							continue;
						};
						path.push((pos, 0.0));
					}
				}
			}
			
			let paint = Paint::default();
			for points in &*paths {
				let len = points.len();
				if len == 1 {
					error!("Length should be at least 2");
					continue
				}
				
				let mut mesh = vertices::Builder::new(
					VertexMode::TriangleStrip,
					points.len() * 2,
					0,
					vertices::BuilderFlags::HAS_COLORS,
				);
				
				let (start, start_force) = points[0];
				let (end, _) = points[1];
				let start = Point::new(start.x as f32, start.y as f32);
				let end = Point::new(end.x as f32, end.y as f32);
				let dir = end - start;
				let mut normal = Point::new(-dir.y, dir.x);
				normal.normalize();
				
				let width_percent = if VARY_WIDTH.load(Relaxed) { start_force as f32 } else { 1.0 };
				let start_offset = normal * width_percent * 4.0;
				let verts = mesh.positions();
				verts[0] = start + start_offset;
				verts[1] = start - start_offset;
				let alpha_percent = if VARY_ALPHA.load(Relaxed) { start_force as f32 } else { 1.0 };
				let start_color = Color::from_argb((255.0 * start_force) as u8, 0, 0, 0);
				let colors = mesh.colors().unwrap();
				colors[0] = start_color;
				colors[1] = start_color;
				
				for (prev_idx, [(prev, _), (next, next_force)]) in points.array_windows().copied().enumerate() {
					let i3 = (prev_idx + 1) * 2;
					let i4 = i3 + 1;
					let prev = Point::new(prev.x as f32, prev.y as f32);
					let next = Point::new(next.x as f32, next.y as f32);
					
					let dir = next - prev;
					let mut normal = Point::new(-dir.y, dir.x);
					normal.normalize();
					let width_percent = if VARY_WIDTH.load(Relaxed) { next_force as f32 } else { 1.0 };
					let next_offset = normal * width_percent * 4.0;
					let p3 = next - next_offset;
					let p4 = next + next_offset;
					let alpha_percent = if VARY_ALPHA.load(Relaxed) { next_force as f32 } else { 1.0 };
					let a2 = (255.0 * alpha_percent) as u8;
					let next_color = Color::from_argb(a2, 0, 0, 0);
					let verts = mesh.positions();
					verts[i3] = p3;
					verts[i4] = p4;
					let colors = mesh.colors().unwrap();
					colors[i3] = next_color;
					colors[i4] = next_color;
				};
				
				let verts = mesh.detach();
				canvas.draw_vertices(&verts, BlendMode::Modulate, &paint);
			}
		})
	});

	// FIXME: Pen is causing both Touch and Mouse start and end events,
	//    resulting in 0-length path at start and duplicate end point

	let on_touch = |e: TouchEvent| {
		let TouchData {
			element_coordinates: pos,
			finger_id: _,
			force,
			phase,
			..
		} = **e;
		let force = if let Some(force) = force {
			let force = force.normalized();
			if force < 0.0001 && phase != TouchPhase::Ended {
				// FIXME: This is necessary for pen support, but I suspect it would break normal touch
				// 		on devices without force support.
				return;
			}
			force
		} else {
			// FIXME: This is necessary for pen support on Windows, but I suspect it would break normal touch
			// 		on devices without force support.
			return;
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
				.send(dbg!(PathMsg::Start(e.element_coordinates, 1.0)))
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
	let end_path = |e: PointerEvent| {
		if matches!(
			e.point_type,
			PointerType::Mouse {
				trigger_button: Some(MouseButton::Left)
			}
		) {
			pen_down.set(false);
			channel
				.write()
				.0
				.send(dbg!(PathMsg::End(e.element_coordinates)))
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
			onpointerup: end_path,
			Canvas {
				canvas: canvas,
				width: "100%",
				height: "100%",
			}
		}
	)
}
