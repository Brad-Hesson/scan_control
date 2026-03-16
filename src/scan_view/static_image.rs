use image_compute::image_compute::FitType;

use crate::{
    components::image_menu::{ImageMenu, NormType},
    scan_view::{ImageEncoder, ScanImage},
};

pub struct StaticImage {
    pub image_data: ScanImage,
    pub fit_type: FitType,
    pub norm_type: NormType,
    pub std_dev: f32,
    pub name: String,
}
impl StaticImage {
    pub fn update_texture(&self, image_encoder: &ImageEncoder) {
        self.image_data.write_texture(image_encoder, self.fit_type);
    }
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl ImageMenu for StaticImage {
    fn fit_type_mut(&mut self) -> &mut FitType {
        &mut self.fit_type
    }

    fn image_data_mut(&mut self) -> &mut ScanImage {
        &mut self.image_data
    }

    fn norm_type_mut(&mut self) -> &mut crate::components::image_menu::NormType {
        &mut self.norm_type
    }

    fn std_dev_mut(&mut self) -> &mut f32 {
        &mut self.std_dev
    }
}
