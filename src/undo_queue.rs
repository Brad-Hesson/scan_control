struct StateModifier<S> {
    undo: Box<dyn Fn(&mut S)>,
    redo: Box<dyn Fn(&mut S)>,
}

pub struct UndoQueue<S> {
    queue: Vec<StateModifier<S>>,
    index: usize,
}
impl<S> UndoQueue<S> {
    pub fn new() -> Self {
        Self {
            queue: Vec::new(),
            index: 0,
        }
    }
    pub fn push(
        &mut self,
        app_state: &mut S,
        redo: impl Fn(&mut S) + 'static,
        undo: impl Fn(&mut S) + 'static,
    ) {
        self.queue.truncate(self.index);
        redo(app_state);
        self.queue.push(StateModifier {
            undo: Box::new(undo),
            redo: Box::new(redo),
        });
        self.index += 1;
    }
    pub fn undo(&mut self, app_state: &mut S) {
        if self.index == 0 {
            return;
        }
        self.index -= 1;
        let f = &self.queue[self.index].undo;
        f(app_state);
    }
    pub fn redo(&mut self, app_state: &mut S) {
        if self.index == self.queue.len() {
            return;
        }
        let f = &self.queue[self.index].redo;
        f(app_state);
        self.index += 1;
    }
}
