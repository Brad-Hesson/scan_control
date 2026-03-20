use std::sync::Arc;

use nanonis_tcp::ScanDir;

#[derive(Clone)]
pub struct BufferState {
    pub size: [usize; 2],
    pub buf_f: Arc<Vec<f32>>,
    pub buf_b: Arc<Vec<f32>>,
    pub scan_dir: ScanDir,
}
