/**
 * This file is based on Section 2.3 of "Asynchronous Programming in Rust" (https://github.com/rust-lang/async-book).
 * Following is the license text of the original book.
 *
 *     MIT License
 *     
 *     Copyright (c) 2018 Aaron Turon
 *     
 *     Permission is hereby granted, free of charge, to any person obtaining a copy
 *     of this software and associated documentation files (the "Software"), to deal
 *     in the Software without restriction, including without limitation the rights
 *     to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 *     copies of the Software, and to permit persons to whom the Software is
 *     furnished to do so, subject to the following conditions:
 *     
 *     The above copyright notice and this permission notice shall be included in all
 *     copies or substantial portions of the Software.
 *     
 *     THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 *     IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 *     FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 *     AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 *     LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 *     OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 *     SOFTWARE.
 */
use core::{
    pin::Pin,
    task::{Context, Poll, Waker},
};

use alloc::{collections::VecDeque, sync::Arc, vec::Vec};
use futures::{future::BoxFuture, task::ArcWake, Future, FutureExt};

use crate::memory_manager::Mutex;

pub struct Receiver<T> {
    queue: Arc<Mutex<VecDeque<T>>>,
    waker: Arc<Mutex<Option<Waker>>>,
}

impl<T> Receiver<T> {
    pub fn receive(&self) -> Option<T> {
        let mut queue = self.queue.lock();
        queue.pop_front()
    }

    pub fn receive_async(&self) -> Recv<'_, T> {
        Recv { receiver: self }
    }
    pub fn has_content(&self) -> bool {
        !self.queue.lock().is_empty()
    }
}

pub struct Recv<'a, T> {
    receiver: &'a Receiver<T>,
}

impl<'a, T> Future for Recv<'a, T> {
    type Output = T;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.receiver.queue.lock().pop_front() {
            Some(val) => Poll::Ready(val),
            None => {
                *self.receiver.waker.lock() = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

#[derive(Clone)]
pub struct Sender<T> {
    queue: Arc<Mutex<VecDeque<T>>>,
    waker: Arc<Mutex<Option<Waker>>>,
}
impl<T> Sender<T> {
    pub fn send(&self, value: T) {
        self.queue.lock().push_back(value);
        if let Some(w) = self.waker.lock().take() {
            w.wake();
        }
    }
}

pub fn new_channel<T>() -> (Sender<T>, Receiver<T>) {
    let queue = Arc::new(Mutex::new(VecDeque::new()));
    let waker = Arc::new(Mutex::new(None));
    (
        Sender {
            queue: queue.clone(),
            waker: waker.clone(),
        },
        Receiver { queue, waker },
    )
}

struct Task<'a, T> {
    future: Mutex<Option<BoxFuture<'a, T>>>,
    sender: Sender<Arc<Self>>,
}

impl<'a, T> Task<'a, T> {
    fn exec(self: Arc<Self>) -> Option<T> {
        let waker = futures::task::waker_ref(&self);
        let mut cx = Context::from_waker(&waker);

        let mut future_slot = self.future.lock();

        if let Some(ref mut future) = *future_slot {
            let result = future.as_mut().poll(&mut cx);
            if let Poll::Ready(result) = result {
                future_slot.take();
                Some(result)
            } else {
                None
            }
        } else {
            None
        }
    }
}

impl<'a, T> ArcWake for Task<'a, T> {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        arc_self.sender.send(arc_self.clone());
    }
}

pub struct Executor<'a, E> {
    task_queue: Receiver<Arc<Task<'a, E>>>,
}

#[derive(Debug)]
pub struct NoMoreTask;
impl<'a, E> Executor<'a, E> {
    pub fn process_next_task(&mut self) -> Result<Option<E>, NoMoreTask> {
        if let Some(task) = self.task_queue.receive() {
            Ok(task.exec())
        } else {
            Err(NoMoreTask)
        }
    }

    pub fn has_next_task(&self) -> bool {
        self.task_queue.has_content()
    }
}

pub struct Spawner<'a, E> {
    sender: Sender<Arc<Task<'a, E>>>,
}

impl<'a, E> Spawner<'a, E> {
    pub fn spawn(&self, future: impl Future<Output = E> + Send + 'a) {
        self.sender.send(Arc::new(Task {
            future: Mutex::new(Some(future.boxed::<'a>())),
            sender: self.sender.clone(),
        }));
    }
}

pub fn new_executor_and_spawner<'a, E>() -> (Executor<'a, E>, Spawner<'a, E>) {
    let (sender, receiver) = new_channel();
    (
        Executor {
            task_queue: receiver,
        },
        Spawner { sender },
    )
}

#[derive(Clone)]
pub struct BroadcastReceiver {
    flag: Arc<Mutex<bool>>,
    wakers: Arc<Mutex<Vec<Waker>>>,
}

impl Future for BroadcastReceiver {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if *self.flag.lock() {
            Poll::Ready(())
        } else {
            self.wakers.lock().push(cx.waker().clone());
            Poll::Pending
        }
    }
}

pub struct BroadcastSender {
    flag: Arc<Mutex<bool>>,
    wakers: Arc<Mutex<Vec<Waker>>>,
}

impl BroadcastSender {
    pub fn send(&self) {
        *self.flag.lock() = true;
        (*self.wakers.lock()).drain(..).for_each(|w| w.wake());
    }
}

pub fn new_broadcast_channel() -> (BroadcastReceiver, BroadcastSender) {
    let flag = Arc::new(Mutex::new(false));
    let wakers = Arc::new(Mutex::new(Vec::new()));
    (
        BroadcastReceiver {
            flag: flag.clone(),
            wakers: wakers.clone(),
        },
        BroadcastSender { flag, wakers },
    )
}
