use std::ops::RangeInclusive;

use egui::{emath, DragValue};

use crate::components::EngFmt;

pub fn si_drag_value<'a>(value: &'a mut impl emath::Numeric) -> DragValue<'a> {
    let speed = 10f64.powf((value.to_f64().abs().log10()).floor().max(-15.) - 2.);
    DragValue::new(value)
        .custom_formatter(formatter)
        .custom_parser(parser)
        .speed(speed)
        .range(-1e3..=1e3)
}

fn formatter(val: f64, _precision: RangeInclusive<usize>) -> String {
    format!("{:.2}", EngFmt(val))
}

fn parser(mut s: &str) -> Option<f64> {
    s = s.trim_ascii();
    let suffix = s.chars().next_back()?;
    let mul = if let Some(mul) = EngFmt::<f64>::multplier_for_char(suffix) {
        s = &s[..s.len() - suffix.len_utf8()];
        s = s.trim_ascii_end();
        mul
    } else {
        1.
    };
    let base_val = s.parse::<f64>().ok()?;
    Some(base_val * mul)
}
