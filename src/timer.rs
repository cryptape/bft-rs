// CITA
// Copyright 2016-2017 Cryptape Technologies LLC.

// This program is free software: you can redistribute it
// and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation,
// either version 3 of the License, or (at your option) any
// later version.

// This program is distributed in the hope that it will be
// useful, but WITHOUT ANY WARRANTY; without even the implied
// warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR
// PURPOSE. See the GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <http://www.gnu.org/licenses/>.

use min_max_heap::MinMaxHeap;

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use bft::Step;

#[derive(Debug, Clone)]
pub struct TimeoutInfo {
    pub timeval: Instant,
    pub height: usize,
    pub round: usize,
    pub step: Step,
}

// define a waittimer channel
pub struct WaitTimer {
    timer_seter: Receiver<TimeoutInfo>,
    timer_notify: Sender<TimeoutInfo>,
}

impl WaitTimer {
    pub fn new(ts: Sender<TimeoutInfo>, rs: Receiver<TimeoutInfo>) -> WaitTimer {
        WaitTimer {
            timer_notify: ts,
            timer_seter: rs,
        }
    }

    pub fn start(&self) {
        let mut timer_heap = MinMaxHeap::new();
        let mut timeout_info = HashMap::new();

        loop {
            // take the peek of the min-heap-timer sub now as the sleep time otherwise set timeout as 100
            let timeout = if !timer_heap.is_empty() {
                if *timer_heap.peek_min().unwrap() > Instant::now() {
                    *timer_heap.peek_min().unwrap() - Instant::now()
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
