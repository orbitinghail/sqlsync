use super::layer::LayerId;

pub struct Cursor {
    layer_id: LayerId,
    frame_idx: usize,
}

impl Cursor {
    pub fn new() -> Self {
        Self {
            layer_id: 0,
            frame_idx: 0,
        }
    }
}