//! Shared application state (kept tiny for M0).

pub struct App {
    /// Number of notifications spawned (for demo content).
    pub tick_count: u32,
}

impl App {
    pub fn new() -> Self {
        Self { tick_count: 0 }
    }
}
