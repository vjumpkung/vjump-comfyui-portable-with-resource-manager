use std::{
    future::Future,
    pin::pin,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    thread,
};

struct ThreadWake(thread::Thread);

impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

pub fn block_on<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(ThreadWake(thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = pin!(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => thread::park(),
        }
    }
}

#[test]
fn executor_handles_a_pending_future_woken_from_another_thread() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let ready = Arc::new(AtomicBool::new(false));
    let mut started = false;
    block_on(std::future::poll_fn(|context| {
        if ready.load(Ordering::Acquire) {
            return Poll::Ready(());
        }
        if !started {
            started = true;
            let ready = ready.clone();
            let waker = context.waker().clone();
            thread::spawn(move || {
                ready.store(true, Ordering::Release);
                waker.wake();
            });
        }
        Poll::Pending
    }));
}
