#![cfg_attr(
	all(not(debug_assertions), target_os = "windows"),
	windows_subsystem = "windows"
)]

use freya::prelude::*;

mod drawing;

fn main() {
	launch_cfg(
		drawing::app,
		LaunchConfig::<()>::builder()
			.with_title("See Augmented")
			.build(),
	);
}
