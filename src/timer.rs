use crate::algorithm::Step;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crossbeam::crossbeam_channel::{Receiver, Sender};
use min_max_heap::MinMaxHeap;

/// Timer infomation.
#[derive(Debug, Clone)]
pub(crate) struct TimeoutInfo {
    /// A timeval of a timer.
    pub(crate) timeval: Instant,
    /// The height of the timer.
    pub(crate) height: u64,
    /// The round of the timer.
    pub(crate) round: u64,
    /// The step of the timer.
    pub(crate) step: Step,
}

/// Sender and receiver of a timeout infomation channel.
pub(crate) struct WaitTimer {
    timer_seter: Receiver<TimeoutInfo>,
    timer_notify: Sender<TimeoutInfo>,
}

impl WaitTimer {
    /// A function to create a new timeout infomation channel.
    pub(crate) fn new(ts: Sender<TimeoutInfo>, rs: Receiver<TimeoutInfo>) -> WaitTimer {
        WaitTimer {
            timer_notify: ts,
            timer_seter: rs,
        }
    }

    /// A function to start a timer.
    pub(crate) fn start(&self) {
        let mut timer_heap = MinMaxHeap::new();
        let mut timeout_info = HashMap::new();

        loop {
            // take the peek of the min-heap-timer sub now as the sleep time otherwise set timeout as 100
            let timeout = if !timer_heap.is_empty() {
                let now = Instant::now();
                if *timer_heap.peek_min().unwrap() > now {
                    *timer_heap.peek_min().unwrap() - now
                } else {
                    Duration::new(0, 0)
                }
            } else {
                Duration::from_secs(100)
            };

            let set_time = self.timer_seter.recv_timeout(timeout);

            // put the timeval into a timerheap
            // put the TimeoutInfo into a hashmap, K: timeval  V: TimeoutInfo
            if set_time.is_ok() {
                let time_out = set_time.unwrap();
                timer_heap.push(time_out.timeval);
                timeout_info.insert(time_out.timeval, time_out);
            }

            if !timer_heap.is_empty() {
                let now = Instant::now();

                // if some timers are set as the same time, send timeout messages and pop them
                while !timer_heap.is_empty() && now >= timer_heap.peek_min().cloned().unwrap() {
                    self.timer_notify
                        .send(timeout_info.remove(&timer_heap.pop_min().unwrap()).unwrap())
                        .unwrap();
                }
            }
        }
    }
}
