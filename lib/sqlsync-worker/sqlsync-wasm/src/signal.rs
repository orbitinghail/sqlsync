use std::hash::Hash;
use std::{cell::RefCell, collections::HashSet, rc::Rc};

use event_listener::Event;
use sqlsync::local::Signal;

pub struct SignalRouter<S: Copy + Hash + Eq> {
    shared: Rc<RefCell<Shared<S>>>,
}

impl<S: Copy + Hash + Eq> SignalRouter<S> {
    pub fn new() -> Self {
        Self { shared: Rc::new(RefCell::new(Shared::new())) }
    }

    pub fn emitter(&self, signal: S) -> SignalEmitter<S> {
        SignalEmitter { signal, shared: self.shared.clone() }
    }

    pub async fn listen(&self) -> Vec<S> {
        let listener = {
            let mut shared = self.shared.borrow_mut();

            // grab a listener before checking signals
            // this ensures that we don't miss a signal
            let listener = shared.event.listen();

            let signals = shared.pop_all();
            if !signals.is_empty() {
                // we already have signals to return
                return signals;
            }
            listener
        };

        // wait for a signal emitter to emit
        listener.await;

        // after the listener fires, we should have signals to return
        self.shared.borrow_mut().pop_all()
    }
}

struct Shared<S: Copy + Hash + Eq> {
    signals: HashSet<S>,
    event: Event,
}

impl<S: Copy + Hash + Eq> Shared<S> {
    fn new() -> Self {
        Self { signals: HashSet::new(), event: Event::new() }
    }

    fn emit(&mut self, signal: S) {
        if self.signals.insert(signal) {
            // only wake up a listener if we added a new signal
            self.event.notify(usize::MAX);
        }
    }

    fn pop_all(&mut self) -> Vec<S> {
        self.signals.drain().collect()
    }
}

pub struct SignalEmitter<S: Copy + Hash + Eq> {
    signal: S,
    shared: Rc<RefCell<Shared<S>>>,
}

impl<S: Copy + Hash + Eq> Signal for SignalEmitter<S> {
    fn emit(&mut self) {
        self.shared.borrow_mut().emit(self.signal)
    }
}
