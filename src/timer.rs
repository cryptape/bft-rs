use crate::error::{handle_err, BftError};
use crate::objects::Step;
use crate::{Height, Round};

use std::cmp::{Ord, Ordering, PartialOrd};
use std::time::{Duration, Instant};

use crossbeam::crossbeam_channel::{Receiver, Sender};
use min_max_heap::MinMaxHeap;
use rlp::{Decodable, DecoderError, Encodable, Prototype, Rlp, RlpStream};

/// Timer infomation.
#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct TimeoutInfo {
    /// A timestamp of a timer.
    pub(crate) timestamp: Instant,

    pub(crate) duration: u64,
    /// The height of the timer.
    pub(crate) height: Height,
    /// The round of the timer.
    pub(crate) round: Round,
    /// The step of the timer.
    pub(crate) step: Step,
}

impl PartialOrd for TimeoutInfo {
    fn partial_cmp(&self, other: &TimeoutInfo) -> Option<Ordering> {
        self.timestamp.partial_cmp(&other.timestamp)
    }
}

impl Ord for TimeoutInfo {
    fn cmp(&self, other: &TimeoutInfo) -> Ordering {
        self.timestamp.cmp(&other.timestamp)
    }
}

impl Encodable for TimeoutInfo {
    fn rlp_append(&self, s: &mut RlpStream) {
        let step: u8 = self.step.into();
        s.begin_list(4)
            .append(&self.duration)
            .append(&self.height)
            .append(&self.round)
            .append(&step);
    }
}

impl Decodable for TimeoutInfo {
    fn decode(r: &Rlp) -> Result<Self, DecoderError> {
        match r.prototype()? {
            Prototype::List(4) => {
                let duration: u64 = r.val_at(0)?;
                let height: Height = r.val_at(1)?;
                let round: Round = r.val_at(2)?;
                let step: u8 = r.val_at(3)?;
                let step: Step = Step::from(step);
                Ok(TimeoutInfo {
                    timestamp: Instant::now() + Duration::from_nanos(duration),
                    duration,
                    height,
                    round,
                    step,
                })
            }
            _ => Err(DecoderError::RlpInconsistentLengthAndData),
        }
    }
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
        let mut timer_heap = MinMaxHeap::<TimeoutInfo>::new();

        loop {
            // take the peek of the min-heap-timer sub now as the sleep time otherwise set timeout as 100
            let timeout = if !timer_heap.is_empty() {
                let peek_min_time = timer_heap.peek_min().unwrap().timestamp;
                let now = Instant::now();
                if peek_min_time > now {
                    peek_min_time - now
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
                timer_heap.push(time_out);
            }

            if !timer_heap.is_empty() {
                let now = Instant::now();

                // if some timers are set as the same time, send timeout messages and pop them
                while !timer_heap.is_empty()
                    && now >= timer_heap.peek_min().cloned().unwrap().timestamp
                {
                    let time_info = timer_heap.pop_min().unwrap();
                    handle_err(self.timer_notify.send(time_info).map_err(|e| {
                        BftError::SendMsgErr(format!("send time notification failed with {:?}", e))
                    }));
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_time_out_info_rlp() {
        let time_out_info = TimeoutInfo {
            timestamp: Instant::now(),
            duration: 10000000u64,
            height: 1888787u64,
            round: 23u64,
            step: Step::Commit,
        };
        let encode = rlp::encode(&time_out_info);
        let decode: TimeoutInfo = rlp::decode(&encode).unwrap();
        assert_eq!(time_out_info.height, decode.height);
    }
}
