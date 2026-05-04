pub fn command_channel<T, R>() -> (CommandChannelSender<T, R>, CommandChannelReciever<T, R>) {
    let (send_tx, send_rx) = crossbeam::channel::unbounded();
    let (recv_tx, recv_rx) = crossbeam::channel::unbounded();
    (
        CommandChannelSender {
            tx: send_tx,
            rx: recv_rx,
        },
        CommandChannelReciever {
            tx: recv_tx,
            rx: send_rx,
        },
    )
}

pub struct CommandChannelSender<T, R> {
    tx: crossbeam::channel::Sender<T>,
    rx: crossbeam::channel::Receiver<R>,
}
impl<T, R> CommandChannelSender<T, R> {
    pub fn send(&self, arg: T) {
        self.tx.send(arg).unwrap();
    }
    pub fn poll_complete(&self) -> Option<R> {
        self.rx.try_recv().ok()
    }
}
impl<T, R> Clone for CommandChannelSender<T, R>{
    fn clone(&self) -> Self {
        Self { tx: self.tx.clone(), rx: self.rx.clone() }
    }
}

pub struct CommandChannelReciever<T, R> {
    tx: crossbeam::channel::Sender<R>,
    rx: crossbeam::channel::Receiver<T>,
}
impl<T, R> CommandChannelReciever<T, R> {
    pub fn try_recv(&self) -> Option<T> {
        self.rx.try_recv().ok()
    }
    pub fn send_response(&self, resp: R) {
        self.tx.send(resp).unwrap()
    }
}

impl<T, R> Clone for CommandChannelReciever<T, R>{
    fn clone(&self) -> Self {
        Self { tx: self.tx.clone(), rx: self.rx.clone() }
    }
}