use std::fmt::Display;

#[repr(transparent)]
pub struct EngFmt(pub f32);
impl Display for EngFmt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mag = (self.0.abs().log10() / 3.).floor();
        let scaled = self.0 / (10f32).powf(mag * 3.);
        let suf = match mag as i32 {
            4 => Some("T"),
            3 => Some("G"),
            2 => Some("M"),
            1 => Some("k"),
            0 => Some(""),
            -1 => Some("m"),
            -2 => Some("μ"),
            -3 => Some("n"),
            -4 => Some("p"),
            -5 => Some("f"),
            _ => None,
        };
        if let Some(suf) = suf {
            f32::fmt(&scaled, f)?;
            write!(f, " {}", suf)?;
        } else if self.0 == 0. {
            f32::fmt(&self.0, f)?;
            write!(f, " m")?;
        } else {
            f32::fmt(&self.0, f)?;
            write!(f, " m")?;
        }
        Ok(())
    }
}
