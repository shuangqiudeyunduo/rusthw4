use std::collections::VecDeque;
use scoped_tls::scoped_thread_local;
use async_std::task::spawn;
use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;
use std::task::Context;
use std::task::Poll;
use std::task::Waker;
use std::sync::Condvar;
use std::task::Wake;
use std::cell::RefCell;
use futures::future::BoxFuture;

struct Task {
    coming:RefCell<BoxFuture<'static, ()>>,
    cue: Arc<Signal>,
}
unsafe impl Send for Task {}
unsafe impl Sync for Task {}
impl Wake for Task {
    fn wake(self:Arc<Self>) {
        RUNNABLE.with(|runnable| runnable.lock().unwrap().push_back(self.clone()));
        self.cue.notify();
    }
}

enum Status {
    Empty,
    Notified,
    Waiting,
}

struct Signal {
    status: Mutex<Status>,
    condv: Condvar,
}
impl Signal {
    fn new() ->Self {
        Self {
            status: Mutex::new(Status::Empty), 
            condv: Condvar::new(),
        }
    }

    fn wait (&self) {
        let mut status0 = self.status.lock().unwrap();
        match *status0 {
            Status::Empty => {
                *status0 = Status::Waiting;
                while let Status::Waiting = *status0 {
                    status0 = self.condv.wait(status0).unwrap();
                }
            }
            Status::Notified => *status0 = Status::Empty,
            Status::Waiting => {
                panic!("mutiple wait");
            }
        }
    }

    fn notify(&self) {
        let mut status0 = self.status.lock().unwrap();
        match *status0 {
            Status::Empty => *status0 = Status::Notified,
            Status::Notified => {}, 
            Status::Waiting => {
                *status0 = Status::Empty;
                self.condv.notify_one();
            }
        }
    }
}
impl Wake for Signal {
    fn wake (self: std::sync::Arc<Self>) {
        self.notify();
    }
}

async fn demo() {
    let (a1,a2) = async_channel::bounded::<()>(1);
    spawn(demo2(a1));
    let _ = a2.recv().await;
    println!("Hello world again!");
}

async fn demo2(a3:async_channel::Sender<()>) {
    let _ = a3.send(()).await;
    println!("Hello world!");
}

scoped_thread_local!(static SIGNAL:Arc<Signal>);
scoped_thread_local!(static RUNNABLE:Mutex<VecDeque<Arc<Task>>>);

fn block_on<F:Future>(future:F) -> F::Output {
    let mut main_fut = std::pin::pin!(future);
    let signal = Arc::new(Signal::new());
    let waker = Waker::from(signal.clone());
    let mut main_cx = Context::from_waker(&waker);
    let runnable = Mutex::new(VecDeque::with_capacity(1024));

    SIGNAL.set(&signal, || {
        RUNNABLE.set(&runnable,|| loop {
            if let Poll::Ready(output) = main_fut.as_mut().poll(&mut main_cx) {
                return output;
            }
            while let Some(task) = runnable.lock().unwrap().pop_back() {
                let waker = Waker::from(task.clone());
                let mut cx = Context::from_waker(&waker);
                let _ = task.coming.borrow_mut().as_mut().poll(&mut cx);
            }
            signal.wait();
        })
    })
}

fn main(){
    let trys = demo();
    block_on(trys);
}