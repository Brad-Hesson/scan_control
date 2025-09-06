use std::any::Any;

struct StateModifier<S> {
    redo: Box<dyn Fn(&mut S, &mut Box<dyn Any>)>,
    undo: Box<dyn Fn(&mut S, &mut Box<dyn Any>)>,
    user_data: Box<dyn Any>,
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
    pub fn push<T: Any>(
        &mut self,
        app_state: &mut S,
        user_data: T,
        redo: impl Fn(&mut S, &mut T) + 'static,
        undo: impl Fn(&mut S, &mut T) + 'static,
    ) {
        self.queue.truncate(self.index);
        let mut user_data: Box<dyn Any> = Box::new(user_data);
        let redo = move |state: &mut S, data: &mut Box<dyn Any>| {
            redo(state, data.downcast_mut().unwrap());
        };
        let undo = move |state: &mut S, data: &mut Box<dyn Any>| {
            undo(state, data.downcast_mut().unwrap());
        };
        redo(app_state, &mut user_data);
        self.queue.push(StateModifier {
            undo: Box::new(undo),
            redo: Box::new(redo),
            user_data,
        });
        self.index += 1;
    }
    pub fn undo(&mut self, app_state: &mut S) {
        if self.index == 0 {
            return;
        }
        self.index -= 1;
        let entry = &mut self.queue[self.index];
        (&entry.undo)(app_state, &mut entry.user_data);
    }
    pub fn redo(&mut self, app_state: &mut S) {
        if self.index == self.queue.len() {
            return;
        }
        let entry = &mut self.queue[self.index];
        (&entry.redo)(app_state, &mut entry.user_data);
        self.index += 1;
    }
}
