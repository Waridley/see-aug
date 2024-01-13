use arc_swap::{ArcSwapOption, RefCnt};
use dioxus::{
	core::{Element, Scope},
	hooks::{use_memo, use_ref, use_state},
};
use freya::prelude::{mouse::MouseButton, pointer::PointerType, touch::TouchPhase, *};
use log::{error, info};
use skia_safe::{
	vertices, vertices::VertexMode, wrapper::PointerWrapper, BlendMode, Canvas, Color, Paint,
	Point, Vertices,
};
use std::{
	sync::{
		atomic::{AtomicBool, AtomicU64, Ordering::Relaxed},
		Arc, Mutex,
	},
	time::{Duration, Instant},
};

type Boxcar<T> = boxcar::Vec<T>;

#[derive(Debug, Copy, Clone, PartialEq)]
enum PathMsg {
	Start(CursorPoint, f64),
	Move(CursorPoint, f64),
	End(CursorPoint),
}

// TODO: Put these in a configuration struct
static VARY_WIDTH: AtomicBool = AtomicBool::new(true);
static VARY_ALPHA: AtomicBool = AtomicBool::new(false);
static DRAW_INTERVAL_MILLIS: AtomicU64 = AtomicU64::new(33);

pub fn app(cx: Scope) -> Element {
	let last_update = use_state(cx, || Instant::now());

	let pen_down = use_state(cx, || false);

	let pipeline = use_ref(cx, || Arc::new(StrokePipeline::new()));
	let dirty = use_state(cx, || ());

	let canvas = use_canvas(cx, dirty, |_| {
		let pipeline = pipeline.read().clone();
		last_update.set(Instant::now());
		Box::new(move |canvas, _fonts, _area| {
			info!("drawing...");
			pipeline.draw(canvas);
		})
	});

	let tx = use_memo(cx, (), |_| {
		let (tx, rx) = std::sync::mpsc::channel();
		let pl = pipeline.with(|pl| pl.clone());
		tokio::spawn(async move {
			loop {
				if let Ok(msg) = rx.recv() {
					pl.message(msg);
				}
				tokio::task::yield_now().await;
			}
		});

		tx
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
				// FIXME: This is necessary for pen support on some devices, but I suspect it would break normal touch
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
		tx.send(msg).unwrap();
		if Instant::now().duration_since(*last_update.get())
			> Duration::from_millis(DRAW_INTERVAL_MILLIS.load(Relaxed))
		{
			dirty.modify(|_| ());
		}
	};

	let start_path = |e: MouseEvent| {
		if matches!(e.trigger_button, Some(MouseButton::Left)) {
			pen_down.set(true);
			tx.send(PathMsg::Start(e.element_coordinates, 1.0)).unwrap();
			if Instant::now().duration_since(*last_update.get())
				> Duration::from_millis(DRAW_INTERVAL_MILLIS.load(Relaxed))
			{
				dirty.modify(|_| ());
			}
		}
	};
	let continue_path = |e: MouseEvent| {
		if *pen_down.get() {
			tx.send(PathMsg::Move(e.element_coordinates, 1.0)).unwrap();
		}
		if Instant::now().duration_since(*last_update.get())
			> Duration::from_millis(DRAW_INTERVAL_MILLIS.load(Relaxed))
		{
			dirty.modify(|_| ());
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
			tx.send(PathMsg::End(e.element_coordinates)).unwrap();
			if Instant::now().duration_since(*last_update.get()) > Duration::from_millis(32) {
				dirty.modify(|_| ());
			}
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

#[derive(Debug, Copy, Clone)]
struct Sample {
	/// Position
	pos: CursorPoint,
	/// Pen force
	f: f32,
}

#[derive(Clone, Debug)]
#[repr(transparent)]
struct RefCntVerts(Vertices);

unsafe impl RefCnt for RefCntVerts {
	type Base = std::ffi::c_void; // `SkVertices` but I don't want to depend on `skia_bindings` and have to build them

	fn into_ptr(me: Self) -> *mut Self::Base {
		unsafe { std::mem::transmute(me) }
	}

	fn as_ptr(me: &Self) -> *mut Self::Base {
		unsafe { *(me as *const Self as *const *mut Self::Base) }
	}

	unsafe fn from_ptr(ptr: *const Self::Base) -> Self {
		Self(Vertices::wrap(ptr as _).unwrap())
	}
}

#[derive(Default, Debug)]
struct StrokePipeline {
	paint: Paint,
	rendered: Arc<Mutex<Vec<Vertices>>>,
	pending_quads: ArcSwapOption<Boxcar<Vertices>>,
	in_progress_points: ArcSwapOption<Boxcar<Point>>,
	in_progress_colors: ArcSwapOption<Boxcar<Color>>,
}

impl StrokePipeline {
	fn new() -> Self {
		Self::default()
	}

	fn draw(&self, canvas: &mut Canvas) {
		for stroke in &**self.rendered.lock().unwrap() {
			canvas.draw_vertices(stroke, BlendMode::Modulate, &self.paint);
		}
		for (_, stroke) in self
			.pending_quads
			.load_full()
			.iter()
			.flat_map(|verts| verts.iter())
		{
			canvas.draw_vertices(stroke, BlendMode::Modulate, &self.paint);
		}
	}

	/// Merge all pending quads and already-merged progress into a new `Vertices` object. Don't call too often,
	/// so that an ever-increasing buffer of vertices doesn't keep getting invalidated and re-pushed to the GPU.
	/// Rather, quads can be pushed until there is time to merge them into a larger mesh to reduce draw calls.
	fn finalize_stroke(&self) {
		loop {
			let Some(in_progress_points) = self.in_progress_points.swap(None) else {
				return;
			};
			let in_progress_colors = self
				.in_progress_colors
				.swap(None)
				.expect("colors should exist if positions do");
			let len = in_progress_points.count();
			for i in 0..len {
				// Make sure all boxcar writes are finalized
				if in_progress_points.get(i).is_none() || in_progress_colors.get(i).is_none() {
					continue;
				}
			}
			let mut builder = vertices::Builder::new(
				VertexMode::TriangleStrip,
				len,
				0,
				vertices::BuilderFlags::HAS_COLORS,
			);
			let positions = builder.positions();
			for (i, point) in in_progress_points.iter() {
				if i >= len {
					break;
				}
				positions[i] = *point;
			}
			let Some(colors) = builder.colors() else {
				error!("colors should exist since we passed `BuilderFlags::HAS_COLORS");
				return;
			};
			for (i, color) in in_progress_colors.iter() {
				if i >= len {
					break;
				}
				colors[i] = *color;
			}
			self.rendered.lock().unwrap().push(builder.detach());
			self.pending_quads.store(None);
			break;
		}
	}

	fn message(self: &Arc<Self>, msg: PathMsg) {
		static CAME_FROM: ArcSwapOption<Sample> = ArcSwapOption::const_empty();
		static LAST_SAMPLE: ArcSwapOption<Sample> = ArcSwapOption::const_empty();

		match msg {
			PathMsg::Start(pos, force) => {
				CAME_FROM.store(None);
				LAST_SAMPLE.store(Some(Arc::new(Sample {
					pos,
					f: force as f32,
				})));
				self.in_progress_points.store(Some(Arc::new(Boxcar::new())));
				self.in_progress_colors.store(Some(Arc::new(Boxcar::new())));
			}
			PathMsg::Move(pos, force) => {
				let last = LAST_SAMPLE.load();
				let Some(last) = last.as_ref() else {
					error!("trying to continue path that is not started");
					return;
				};
				let dir = pos - last.pos;
				let mut normal = Point::new(-dir.y as f32, dir.x as f32);
				normal.normalize();

				let prev_dir = if let Some(came_from) = CAME_FROM.load().as_ref() {
					last.pos - came_from.pos
				} else {
					dir
				};
				let mut prev_normal = Point::new(-prev_dir.y as f32, prev_dir.x as f32);
				prev_normal.normalize();

				let ([p1, p2], c1) = Self::verts_for(prev_normal, **last);

				let Some(in_progress_points) = self.in_progress_points.load_full() else {
					error!("trying to continue path with missing points");
					return;
				};
				if in_progress_points.is_empty() {
					// Push the first 2 vertices
					self.push_verts(p1, p2, c1);
				}
				let sample = Sample {
					pos,
					f: force as f32,
				};
				let ([p3, p4], c2) = Self::verts_for(normal, sample);
				if dir.length() > 4.0 {
					// don't draw lines too short
					self.push_verts(p3, p4, c2);
				} else {
					return;
				}
				self.gen_quad([p1, p2, p3, p4], [c1, c1, c2, c2]);
				CAME_FROM.store(Some(last.clone()));
				LAST_SAMPLE.store(Some(Arc::new(sample)));
			}
			PathMsg::End(pos) => {
				let Some(last) = LAST_SAMPLE.swap(None) else {
					error!("trying to end a path that is not started");
					return;
				};
				if !self.in_progress_points.load().is_some() {
					error!("trying to continue path with missing points");
					return;
				};
				let dir = pos - last.pos;
				let mut normal = Point::new(-dir.y as f32, dir.x as f32);
				normal.normalize();

				let ([p1, p2], color1) = Self::verts_for(normal, *last);
				let ([p3, p4], color2) = Self::verts_for(normal, Sample { pos, f: 0.0 });
				self.push_verts(p3, p4, color2);
				self.gen_quad([p1, p2, p3, p4], [color1, color1, color2, color2]);
				let this = self.clone();
				tokio::spawn(async move {
					this.finalize_stroke();
				});
			}
		}
	}

	fn verts_for(normal: Point, Sample { pos, f }: Sample) -> ([Point; 2], Color) {
		let pos = Point::new(pos.x as f32, pos.y as f32);
		let width_percent = if VARY_WIDTH.load(Relaxed) { f } else { 1.0 };
		let alpha_percent = if VARY_ALPHA.load(Relaxed) { f } else { 1.0 };
		let offset = normal * width_percent * 6.0;
		let color = Color::from_argb((255.0 * alpha_percent) as u8, 0, 0, 0);
		([pos + offset, pos - offset], color)
	}

	fn push_verts(&self, p1: Point, p2: Point, color: Color) {
		let points = self.in_progress_points.load();
		let points = points.as_ref().unwrap();
		let colors = self.in_progress_colors.load();
		let colors = colors.as_ref().unwrap();
		points.push(p1);
		points.push(p2);
		colors.push(color);
		colors.push(color);
	}

	fn gen_quad(&self, points: [Point; 4], colors: [Color; 4]) {
		let mut builder = vertices::Builder::new(
			VertexMode::TriangleStrip,
			4,
			0,
			vertices::BuilderFlags::HAS_COLORS,
		);
		builder.positions().clone_from_slice(&points);
		builder
			.colors()
			.expect("colors should exist since we passed `BuilderFlags::HAS_COLORS`")
			.clone_from_slice(&colors);
		let verts = builder.detach();
		// CaS seems to not be possible here unless I figure out the missing trait bounds
		match self.pending_quads.load().as_ref() {
			None => self
				.pending_quads
				.store(Some(Arc::new(boxcar::vec![verts]))),
			Some(pending_quads) => {
				pending_quads.push(verts);
			}
		}
	}
}
