use glam::{DAffine2, DMat3, DVec2, Vec3Swizzles};

pub trait IntoGlam {
    fn to_glam(self) -> glam::DVec2;
}
impl IntoGlam for egui::Pos2 {
    fn to_glam(self) -> glam::DVec2 {
        glam::DVec2 {
            x: self.x as f64,
            y: self.y as f64,
        }
    }
}
impl IntoGlam for egui::Vec2 {
    fn to_glam(self) -> glam::DVec2 {
        glam::DVec2 {
            x: self.x as f64,
            y: self.y as f64,
        }
    }
}

pub trait IntoEgui {
    fn to_egui_pos2(self) -> egui::Pos2;
    fn to_egui_vec2(self) -> egui::Vec2;
}
impl IntoEgui for glam::DVec2 {
    fn to_egui_pos2(self) -> egui::Pos2 {
        egui::Pos2 {
            x: self.x as f32,
            y: self.y as f32,
        }
    }
    fn to_egui_vec2(self) -> egui::Vec2 {
        egui::Vec2 {
            x: self.x as f32,
            y: self.y as f32,
        }
    }
}

pub trait Projection {
    fn project_pos2(&self, pos: DVec2) -> DVec2;
}
impl Projection for DMat3 {
    fn project_pos2(&self, pos: DVec2) -> DVec2 {
        let pos_p = self * pos.extend(1.);
        pos_p.xy() / pos_p.z
    }
}
