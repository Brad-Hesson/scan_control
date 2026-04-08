pub mod backing;
mod live_image;
pub mod queue;
mod scan_area;
mod shared_state;

pub use live_image::LiveImage;
pub use scan_area::ScanArea;

use crate::scan_view::ImageEncoder;
pub mod nanonis;

pub trait Connection {
    fn poll_connected(&mut self, encoder: &ImageEncoder) -> Option<ScanArea>;
    fn update(&mut self, scan_area: &mut ScanArea, encoder: &ImageEncoder) -> Option<LiveImage>;
}
