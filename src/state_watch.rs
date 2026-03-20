use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use parking_lot::{RwLock, RwLockReadGuard};

#[derive(Clone)]
pub struct WatchRw<T> {
    mutex: Arc<RwLock<T>>,
    shared_epoch: Arc<AtomicU64>,
    local_epoch: u64,
}
impl<T> WatchRw<T> {
    pub fn new(val: T) -> Self {
        Self {
            mutex: Arc::new(RwLock::new(val)),
            shared_epoch: Arc::new(AtomicU64::new(1)),
            local_epoch: 0,
        }
    }
    #[inline]
    pub fn peek(&self) -> RwLockReadGuard<'_, T> {
        self.mutex.read()
    }
    #[inline]
    pub fn read(&mut self) -> RwLockReadGuard<'_, T> {
        self.local_epoch = self.shared_epoch.load(Ordering::Relaxed);
        self.mutex.read()
    }
    #[inline]
    pub fn read_new(&mut self) -> Option<RwLockReadGuard<'_, T>> {
        let shared_epoch = self.shared_epoch.load(Ordering::Relaxed);
        if self.local_epoch < shared_epoch {
            self.local_epoch = shared_epoch;
            Some(self.mutex.read())
        } else {
            None
        }
    }
    #[inline]
    pub fn modify(&mut self, mod_fn: impl FnOnce(&mut T)) {
        mod_fn(&mut *self.mutex.write());
        let last_epoch = self.shared_epoch.fetch_add(1, Ordering::Relaxed);
        self.local_epoch = last_epoch + 1;
    }
    #[inline]
    pub fn modify_conditional(
        &mut self,
        check_fn: impl FnOnce(&T) -> bool,
        mod_fn: impl FnOnce(&mut T),
    ) -> bool {
        let mut up_read = self.mutex.upgradable_read();
        let wants_modify = check_fn(&up_read);
        if wants_modify {
            up_read.with_upgraded(mod_fn);
            let last_epoch = self.shared_epoch.fetch_add(1, Ordering::Relaxed);
            self.local_epoch = last_epoch + 1;
        }
        wants_modify
    }
}
