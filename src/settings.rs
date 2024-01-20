

pub struct Settings {
	pub log_level: tracing_subscriber::filter::LevelFilter,
}

impl Default for Settings {
	fn default() -> Self {
		Self {
			log_level: tracing_subscriber::filter::LevelFilter::DEBUG,
		}
	}
}
