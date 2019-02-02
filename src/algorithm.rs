// CITA
// Copyright 2016-2019 Cryptape Technologies LLC.

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

use bincode::{deserialize, serialize, Infinite};
use crossbeam::crossbeam_channel::{unbounded, Receiver, Sender};
use params::BftTimer;
use timer::TimeoutInfo;
use voteset::AuthorityManage;
use voteset::*;
use wal::Wal;

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::convert::{Into, TryFrom, TryInto};
use std::time::{Duration, Instant};

use super::*;

const INIT_HEIGHT: usize = 1;
const INIT_ROUND: usize = 0;
const TIMEOUT_RETRANSE_MULTIPLE: u32 = 15;
const MAX_PROPOSAL_TIME_COEF: usize = 10;

#[derive(Serialize, Deserialize, Debug, PartialEq, PartialOrd, Eq, Clone, Copy, Hash)]
pub enum Step {
    Propose,
    ProposeWait,
    Prevote,
    PrevoteWait,
    Precommit,
    PrecommitWait,
    Commit,
    CommitWait,
}

impl Default for Step {
    fn default() -> Step {
        Step::Propose
    }
}

impl From<u8> for Step {
    fn from(s: u8) -> Step {
        match s {
            0u8 => Step::Propose,
            1u8 => Step::ProposeWait,
            2u8 => Step::Prevote,
            3u8 => Step::PrevoteWait,
            4u8 => Step::Precommit,
            5u8 => Step::PrecommitWait,
            6u8 => Step::Commit,
            7u8 => Step::CommitWait,
            _ => panic!("Invalid step."),
        }
    }
}

pub struct Bft {
    msg_sender: Sender<BftMsg>,
    msg_receiver: Receiver<BftMsg>,
    timer_seter: Sender<TimeoutInfo>,
    timer_notity: Receiver<TimeoutInfo>,

    height: usize,
    round: usize,
    step: Step,
    proposals: Option<Target>,
    proposal: Option<Target>,
    votes: VoteCollector,
    lock_status: Option<LockStatus>,
    // wal_log: Wal,
    last_commit_round: Option<usize>,
    last_commit_proposal: Option<Target>,
    authority_list: Vec<Address>,
    interval: u64,
}

impl Bft {
    pub fn start(s: Sender<BftMsg>, r: Receiver<BftMsg>, timer: BftTimer) {
        let (bft2timer, timer4bft) = unbounded();
        let (timer2bft, bft4timer) = unbounded();
        let engine = Bft::initialize(s, r, bft2timer, bft4timer, timer);
    }

    fn initialize(
        s: Sender<BftMsg>,
        r: Receiver<BftMsg>,
        ts: Sender<TimeoutInfo>,
        tn: Receiver<TimeoutInfo>,
        timer: u64,
    ) -> Bft {
        Bft {
            msg_sender: s,
            msg_receiver: r,
            timer_seter: ts,
            timer_notity: tn,

            height: INIT_HEIGHT,
            round: INIT_ROUND,
            Step: Step::Propose,
            proposals: None,
            proposal: None,
            vote: VoteCollector::new(),
            last_commit_round: None,
            last_commit_proposal: None,
            authority_list: Vec::new(),
            interval: timer,
        }
    }
}
