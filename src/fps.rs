use std::fmt::{Display, Formatter};

#[derive(Debug)]
pub struct FPS {
    #[cfg(not(target_arch = "wasm32"))]
    last_second: std::time::Instant,

    #[cfg(target_arch = "wasm32")]
    last_second: f64,

    value: u32,
    frame_count: u32,
}

impl Display for FPS {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} FPS", self.value)
    }
}

impl FPS {
    pub fn new() -> Self {
        FPS {
            #[cfg(not(target_arch = "wasm32"))]
            last_second: std::time::Instant::now(),

            #[cfg(target_arch = "wasm32")]
            last_second: web_sys::window()
                .and_then(|window| window.performance())
                .unwrap()
                .now(),

            value: 0,
            frame_count: 0,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn update(&mut self) {
        self.frame_count += 1;

        if self.last_second.elapsed() >= std::time::Duration::from_secs(1) {
            self.value = self.frame_count;
            self.frame_count = 0;
            self.last_second = std::time::Instant::now();
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub fn update(&mut self) {
        self.frame_count += 1;

        let now = web_sys::window()
            .and_then(|window| window.performance())
            .unwrap()
            .now();

        if now - self.last_second >= 1000.0 {
            self.value = self.frame_count;
            self.frame_count = 0;
            self.last_second = now;
        }
    }
}
