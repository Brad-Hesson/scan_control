use std::hash::Hash;

use egui::{Ui, WidgetText};

pub trait ComboBoxType: Eq {
    type Ctx;
    fn opt_atoms(&self, ctx: &Self::Ctx) -> impl Into<WidgetText>;
    fn options(ctx: &Self::Ctx) -> impl Iterator<Item = Self>;
}

pub fn combo_box<'s, T: ComboBoxType>(
    ui: &mut Ui,
    id_salt: impl Hash,
    data: &mut T,
    ctx: &T::Ctx,
) -> bool {
    egui::ComboBox::new((id_salt, "combo_box"), "")
        .selected_text(
            T::options(ctx)
                .find(|t| *t == *data)
                .map(|t| t.opt_atoms(ctx).into())
                .unwrap(),
        )
        .show_ui(ui, |ui| {
            T::options(ctx)
                .map(|opt| {
                    let text = opt.opt_atoms(ctx).into();
                    ui.selectable_value(data, opt, text)
                })
                .any(|resp| resp.clicked())
        })
        .inner
        .is_some_and(|clicked| clicked)
}
