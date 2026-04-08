use std::sync::Arc;

use crossbeam::{
    queue::ArrayQueue,
    sync::{Parker, Unparker},
};

pub fn overwrite_queue<T>(cap: usize) -> (OverwriteQueueSender<T>, OverwriteQueueReceiver<T>) {
    let queue = Arc::new(ArrayQueue::new(cap));
    let parker = Parker::new();
    let unparker = parker.unparker().clone();
    (
        OverwriteQueueSender {
            queue: Arc::clone(&queue),
            unparker,
        },
        OverwriteQueueReceiver { queue, parker },
    )
}

#[derive(Clone)]
pub struct OverwriteQueueSender<T> {
    queue: Arc<ArrayQueue<T>>,
    unparker: Unparker,
}
impl<T> OverwriteQueueSender<T> {
    pub fn send(&self, value: T) -> Option<T> {
        let overwrote = self.queue.force_push(value);
        self.unparker.unpark();
        overwrote
    }
}

pub struct OverwriteQueueReceiver<T> {
    queue: Arc<ArrayQueue<T>>,
    parker: Parker,
}
impl<T> OverwriteQueueReceiver<T> {
    pub fn recv(&self) -> T {
        loop {
            if let Some(val) = self.queue.pop() {
                return val;
            }
            self.parker.park();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn test_name() {
        let (tx, rx) = overwrite_queue(1);
        std::thread::spawn(move || loop {
            dbg!(rx.recv());
        });
        tx.send(1);
        std::thread::sleep(Duration::from_secs(1));
        tx.send(2);
        std::thread::sleep(Duration::from_secs(1));
        tx.send(3);
    }
}
