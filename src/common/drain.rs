use std::mem;

use tokio_sync::{mpsc, watch};

use super::{Future, Never, Poll, Pin, task};
use futures_util::FutureExt as _;

// Sentinel value signaling that the watch is still open
enum Action {
    Open,
    // Closed isn't sent via the `Action` type, but rather once
    // the watch::Sender is dropped.
}

pub fn channel() -> (Signal, Watch) {
    let (tx, rx) = watch::channel(Action::Open);
    let (drained_tx, drained_rx) = mpsc::channel(1);
    (
        Signal {
            drained_rx,
            tx,
        },
        Watch {
            drained_tx,
            rx,
        },
    )
}

pub struct Signal {
    drained_rx: mpsc::Receiver<Never>,
    tx: watch::Sender<Action>,
}

pub struct Draining {
    drained_rx: mpsc::Receiver<Never>,
}

#[derive(Clone)]
pub struct Watch {
    drained_tx: mpsc::Sender<Never>,
    rx: watch::Receiver<Action>,
}

#[allow(missing_debug_implementations)]
pub struct Watching<F, FN> {
    future: F,
    state: State<FN>,
    watch: Watch,
}

enum State<F> {
    Watch(F),
    Draining,
}

impl Signal {
    pub fn drain(self) -> Draining {
        // Simply dropping `self.tx` will signal the watchers
        Draining {
            drained_rx: self.drained_rx,
        }
    }
}

impl Future for Draining {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<Self::Output> {
        match ready!(self.drained_rx.poll_recv(cx)) {
            Some(never) => match never {},
            None => Poll::Ready(()),
        }
    }
}

impl Watch {
    pub fn watch<F, FN>(self, future: F, on_drain: FN) -> Watching<F, FN>
    where
        F: Future,
        FN: FnOnce(Pin<&mut F>),
    {
        Watching {
            future,
            state: State::Watch(on_drain),
            watch: self,
        }
    }
}

impl<F, FN> Future for Watching<F, FN>
where
    F: Future,
    FN: FnOnce(Pin<&mut F>),
{
    type Output = F::Output;

    fn poll(mut self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<Self::Output> {
        let me = unsafe { self.get_unchecked_mut() };
        loop {
            match mem::replace(&mut me.state, State::Draining) {
                State::Watch(on_drain) => {
                    let mut recv_fut = me.watch.rx.recv_ref().boxed();

                    match recv_fut.poll_unpin(cx) {
                        Poll::Ready(None) => {
                            // Drain has been triggered!
                            on_drain(unsafe { Pin::new_unchecked(&mut me.future) });
                        },
                        Poll::Ready(Some(_/*State::Open*/)) |
                        Poll::Pending => {
                            me.state = State::Watch(on_drain);
                            return unsafe { Pin::new_unchecked(&mut me.future) }.poll(cx);
                        },
                    }
                },
                State::Draining => {
                    return unsafe { Pin::new_unchecked(&mut me.future) }.poll(cx);
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    // FIXME: re-implement tests with `async/await`, this import should
    // trigger a warning to remind us
    use crate::Error;

    /*
    use futures::{future, Async, Future, Poll};
    use super::*;

    struct TestMe {
        draining: bool,
        finished: bool,
        poll_cnt: usize,
    }

    impl Future for TestMe {
        type Item = ();
        type Error = ();

        fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
            self.poll_cnt += 1;
            if self.finished {
                Ok(Async::Ready(()))
            } else {
                Ok(Async::NotReady)
            }
        }
    }

    #[test]
    fn watch() {
        future::lazy(|| {
            let (tx, rx) = channel();
            let fut = TestMe {
                draining: false,
                finished: false,
                poll_cnt: 0,
            };

            let mut watch = rx.watch(fut, |fut| {
                fut.draining = true;
            });

            assert_eq!(watch.future.poll_cnt, 0);

            // First poll should poll the inner future
            assert!(watch.poll().unwrap().is_not_ready());
            assert_eq!(watch.future.poll_cnt, 1);

            // Second poll should poll the inner future again
            assert!(watch.poll().unwrap().is_not_ready());
            assert_eq!(watch.future.poll_cnt, 2);

            let mut draining = tx.drain();
            // Drain signaled, but needs another poll to be noticed.
            assert!(!watch.future.draining);
            assert_eq!(watch.future.poll_cnt, 2);

            // Now, poll after drain has been signaled.
            assert!(watch.poll().unwrap().is_not_ready());
            assert_eq!(watch.future.poll_cnt, 3);
            assert!(watch.future.draining);

            // Draining is not ready until watcher completes
            assert!(draining.poll().unwrap().is_not_ready());

            // Finishing up the watch future
            watch.future.finished = true;
            assert!(watch.poll().unwrap().is_ready());
            assert_eq!(watch.future.poll_cnt, 4);
            drop(watch);

            assert!(draining.poll().unwrap().is_ready());

            Ok::<_, ()>(())
        }).wait().unwrap();
    }

    #[test]
    fn watch_clones() {
        future::lazy(|| {
            let (tx, rx) = channel();

            let fut1 = TestMe {
                draining: false,
                finished: false,
                poll_cnt: 0,
            };
            let fut2 = TestMe {
                draining: false,
                finished: false,
                poll_cnt: 0,
            };

            let watch1 = rx.clone().watch(fut1, |fut| {
                fut.draining = true;
            });
            let watch2 = rx.watch(fut2, |fut| {
                fut.draining = true;
            });

            let mut draining = tx.drain();

            // Still 2 outstanding watchers
            assert!(draining.poll().unwrap().is_not_ready());

            // drop 1 for whatever reason
            drop(watch1);

            // Still not ready, 1 other watcher still pending
            assert!(draining.poll().unwrap().is_not_ready());

            drop(watch2);

            // Now all watchers are gone, draining is complete
            assert!(draining.poll().unwrap().is_ready());

            Ok::<_, ()>(())
        }).wait().unwrap();
    }
    */
}

