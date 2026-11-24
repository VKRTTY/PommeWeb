//! Platform executor for the same client state machines. Native uses the
//! existing Tokio runtime; browser futures run on the page's event loop.
#[cfg(not(target_arch = "wasm32"))]
pub use tokio::runtime::Runtime;
#[cfg(not(target_arch = "wasm32"))]
pub type Task<T> = tokio::task::JoinHandle<T>;

#[cfg(not(target_arch = "wasm32"))]
pub trait MaybeSend: Send {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + ?Sized> MaybeSend for T {}

#[cfg(target_arch = "wasm32")]
pub trait MaybeSend {}
#[cfg(target_arch = "wasm32")]
impl<T: ?Sized> MaybeSend for T {}

pub fn runtime() -> Runtime {
    #[cfg(not(target_arch = "wasm32"))]
    return tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime");
    #[cfg(target_arch = "wasm32")]
    Runtime
}

#[cfg(not(target_arch = "wasm32"))]
pub use tokio::spawn;

#[cfg(target_arch = "wasm32")]
pub use browser::{Runtime, Task, spawn};

#[cfg(target_arch = "wasm32")]
mod browser {
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    use futures_util::future::{AbortHandle, Abortable};
    use tokio::sync::oneshot;

    pub struct Runtime;

    impl Runtime {
        pub fn spawn<F: Future + 'static>(&self, future: F) -> Task<F::Output> {
            spawn(future)
        }
    }

    #[must_use = "dropping a task detaches it; call abort to cancel it"]
    pub struct Task<T> {
        result: oneshot::Receiver<T>,
        abort: AbortHandle,
    }

    impl<T> Task<T> {
        pub fn abort(&self) {
            self.abort.abort();
        }
    }

    impl<T> Future for Task<T> {
        type Output = Result<T, oneshot::error::RecvError>;

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            Pin::new(&mut self.get_mut().result).poll(cx)
        }
    }

    pub fn spawn<F: Future + 'static>(future: F) -> Task<F::Output> {
        let (send, result) = oneshot::channel();
        let (abort, registration) = AbortHandle::new_pair();
        wasm_bindgen_futures::spawn_local(async move {
            if let Ok(output) = Abortable::new(future, registration).await {
                let _ = send.send(output);
            }
        });
        Task { result, abort }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub mod time {
    pub use tokio::time::timeout;
    pub fn delay_interval(period: std::time::Duration) -> tokio::time::Interval {
        let mut timer = tokio::time::interval(period);
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        timer
    }
}

#[cfg(target_arch = "wasm32")]
pub mod time {
    use std::future::Future;
    use std::pin::{Pin, pin};
    use std::time::Duration;

    use futures_util::future::{Either, select};
    use js_sys::Promise;
    use wasm_bindgen::prelude::*;
    use wasm_bindgen_futures::JsFuture;

    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = globalThis, js_name = setTimeout)]
        fn set_timeout(callback: &js_sys::Function, millis: i32) -> i32;
    }

    pub async fn sleep(duration: Duration) {
        let promise = Promise::new(&mut |resolve, _| {
            set_timeout(&resolve, duration.as_millis().min(i32::MAX as u128) as i32);
        });
        let _ = JsFuture::from(promise).await;
    }

    pub async fn timeout<F: Future>(duration: Duration, future: F) -> Result<F::Output, ()> {
        match select(pin!(future), pin!(sleep(duration))).await {
            Either::Left((result, _)) => Ok(result),
            Either::Right(_) => Err(()),
        }
    }

    pub struct DelayInterval {
        period: Duration,
        pending: Option<Pin<Box<dyn Future<Output = ()>>>>,
    }

    pub fn delay_interval(period: Duration) -> DelayInterval {
        DelayInterval { period, pending: None }
    }

    impl DelayInterval {
        pub async fn tick(&mut self) {
            if let Some(pending) = &mut self.pending {
                pending.await;
            }
            self.pending = Some(Box::pin(sleep(self.period)));
            // Start the next timer now, rather than when a later select first
            // asks for it, and retain it across canceled reads.
            std::future::poll_fn(|cx| {
                let mut pending = self.pending.as_mut().unwrap().as_mut();
                let _ = pending.as_mut().poll(cx);
                std::task::Poll::Ready(())
            }).await;
        }
    }
}
