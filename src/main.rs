#![feature(array_windows)]
#![cfg_attr(
	all(not(debug_assertions), target_os = "windows"),
	windows_subsystem = "windows"
)]

use freya::events::touch::TouchPhase;
use freya::prelude::*;

use pointer::{MouseButton, PointerType};
use skia_safe::{BlendMode, Color, Paint, Path, Point, Vertices};
use std::collections::VecDeque;

use std::sync::{Arc, Mutex};
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
				
				let mut verts = Vec::new();
				let mut colors = Vec::new();
				
				let (start, start_force) = points[0];
				let (end, _) = points[1];
				let start = Point::new(start.x as f32, start.y as f32);
				let end = Point::new(end.x as f32, end.y as f32);
				let dir = end - start;
				let mut normal = Point::new(-dir.y, dir.x);
				normal.normalize();
				
				let start_offset = normal * 6.0;
				verts.push(start + start_offset);
				verts.push(start - start_offset);
				let start_color = Color::from_argb((255.0 * start_force) as u8, 0, 0, 0);
				colors.push(start_color);
				colors.push(start_color);
				
				for [(prev, _), (next, next_force)] in points.array_windows().copied() {
					let prev = Point::new(prev.x as f32, prev.y as f32);
					let next = Point::new(next.x as f32, next.y as f32);
					
					let dir = next - prev;
					let mut normal = Point::new(-dir.y, dir.x);
					normal.normalize();
					let next_force = next_force as f32;
					let next_offset = normal * 6.0;
					let p3 = next - next_offset;
					let p4 = next + next_offset;
					let a2 = (255.0 * next_force) as u8;
					let next_color = Color::from_argb(a2, 0, 0, 0);
					verts.push(p3);
					verts.push(p4);
					colors.push(next_color);
					colors.push(next_color)
				};
				
				let texs = vec![Point::default(); verts.len()];
				let verts = Vertices::new_copy(
					VertexMode::TriangleStrip,
					&verts,
					&texs,
					&colors,
					None
				);
				
				canvas.draw_vertices(&verts, BlendMode::Multiply, &paint);
				
				// let mut path = Path::new();
				// let mut points = points.iter().copied();
				// let mut came_from = Option::<(CursorPoint, f64)>::None;
				// let mut last = points.next().unwrap();
				// for (point, force) in points {
				// 	let start = Point::new(last.0.x as f32, last.0.y as f32);
				// 	let end = Point::new(point.x as f32, point.y as f32);
				//
				// 	let dir = end - start;
				// 	let prev_dir = if let Some(came_from) = came_from {
				// 		start - Point::new(came_from.0.x as f32, came_from.0.y as f32)
				// 	} else {
				// 		dir
				// 	};
				// 	let mut normal = Point::new(-dir.y, dir.x);
				// 	normal.normalize();
				// 	let mut prev_normal = Point::new(-prev_dir.y, prev_dir.x);
				// 	prev_normal.normalize();
				// 	let start_offset = prev_normal * (last.1 as f32) * 6.0;
				// 	let end_offset = normal * (force as f32) * 6.0;
				// 	let p1 = start + start_offset;
				// 	let p2 = start - start_offset;
				// 	let p3 = end - end_offset;
				// 	let p4 = end + end_offset;
				//
				// 	path.add_poly(&[p1, p2, p3, p4], true);
				// 	came_from = Some(last);
				// 	last = (point, force);
				// }
				// canvas.draw_path(&path, &paint);
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
				.send(dbg!(PathMsg::Start(e.element_coordinates, 0.5)))
				.unwrap();
		}
	};
	let continue_path = |e: MouseEvent| {
		if *pen_down.get() {
			channel
				.write()
				.0
				.send(PathMsg::Move(e.element_coordinates, 0.5))
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
