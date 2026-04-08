use std::sync::Arc;

#[derive(Default)]
pub struct ChannelState {
    names: Arc<Box<[String]>>,
    opts: Vec<usize>,
    pub selection: Option<usize>,
}
impl ChannelState {
    pub fn modify_opts(&mut self, mod_fn: impl FnOnce(&mut Vec<usize>)) {
        mod_fn(&mut self.opts);
        if self
            .selection
            .is_none_or(|selection| !self.opts.contains(&selection))
        {
            if self.opts.contains(&30) {
                self.selection = Some(30);
            } else if self.opts.len() > 0 {
                self.selection = Some(self.opts[0]);
            } else {
                self.selection = None;
            }
        }
    }
    pub fn options(&self) -> &[usize] {
        &self.opts
    }
    pub fn write_names(&mut self, names: Arc<Box<[String]>>) {
        self.names = names;
    }
    pub fn channel_opts_names<'s>(&'s self) -> impl Iterator<Item = String> + 's {
        self.opts.iter().map(|opt| self.names[*opt].clone())
    }
    pub fn selected_as_string(&self) -> Option<String> {
        self.selection.map(|sel| self.names[sel].to_string())
    }
    pub fn set_selection_by_name(&mut self, name: &str) {
        let idx = self.names.iter().position(|n| n == name).unwrap();
        self.selection = Some(idx)
    }
    pub fn unit(&self) -> Option<String> {
        self.selected_as_string()
            .and_then(|sel| sel.split(['(', ')']).nth(1).map(str::to_string))
    }
}
