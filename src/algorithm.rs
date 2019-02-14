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
use crossbeam::crossbeam_channel::{unbounded, Receiver, RecvError, Sender};
use log;
use params::BftParams;
use timer::{TimeoutInfo, WaitTimer};
use voteset::VoteCollector;
use voteset::*;
use wal::Wal;

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::convert::{Into, TryFrom, TryInto};
use std::time::{Duration, Instant};
use std::thread;

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
    proposals: Option<Target>, // proposals means the latest proposal given by auth at this height
    proposal: Option<Target>,
    vote: VoteCollector,
    lock_status: Option<LockStatus>,
    // wal_log: Wal,
    last_commit_round: Option<usize>,
    last_commit_proposal: Option<Target>,
    authority_list: Vec<Address>,
    params: BftParams,
}

impl Bft {
    pub fn start(s: Sender<BftMsg>, r: Receiver<BftMsg>, local_address: Address) {
        // define message channel and timeout channel
        let (bft2timer, timer4bft) = unbounded();
        let (timer2bft, bft4timer) = unbounded();

        // start timer module
        let timer_thread = thread::spawn(move || {
            let timer = WaitTimer::new(timer2bft, timer4bft);
            timer.start();
        });

        // start main
        let mut engine = Bft::initialize(s, r, bft2timer, bft4timer, local_address);
        let main_thread = thread::spawn(move || loop {
            let mut get_timer_msg = Err(RecvError);
            let mut get_msg = Err(RecvError);

            select! {
                recv(engine.timer_notity) -> msg => get_timer_msg = msg,
                recv(engine.msg_receiver) -> msg => get_msg = msg,
            }

            if let Ok(ok_timer) = get_timer_msg {
                engine.timeout_process(&ok_timer);
            }

            if let Ok(ok_msg) = get_msg {
                engine.process(ok_msg);
            }
        });

        main_thread.join().unwrap();
        timer_thread.join().unwrap();
    }

    fn initialize(
        s: Sender<BftMsg>,
        r: Receiver<BftMsg>,
        ts: Sender<TimeoutInfo>,
        tn: Receiver<TimeoutInfo>,
        local_address: Target,
    ) -> Self {
        Bft {
            msg_sender: s,
            msg_receiver: r,
            timer_seter: ts,
            timer_notity: tn,

            height: INIT_HEIGHT,
            round: INIT_ROUND,
            step: Step::default(),
            proposals: None,
            proposal: None,
            vote: VoteCollector::new(),
            lock_status: None,
            last_commit_round: None,
            last_commit_proposal: None,
            authority_list: Vec::new(),
            params: BftParams::new(local_address),
        }
    }

    #[inline]
    fn set_timer(&self, duration: Duration, s: Step) {
        let _ = self.timer_seter.send(TimeoutInfo {
            timeval: Instant::now() + duration,
            height: self.height,
            round: self.round,
            step: s,
        });
    }

    #[inline]
    fn send_bft_msg(&self, msg: BftMsg) {
        let _ = self.msg_sender.send(msg);
    }

    #[inline]
    fn cal_above_threshold(&self, count: usize) -> bool {
        count * 3 > self.authority_list.len() * 2
    }

    #[inline]
    fn cal_all_vote(&self, count: usize) -> bool {
        count == self.authority_list.len()
    }

    #[inline]
    fn change_to_step(&mut self, step: Step) {
        self.step = step;
    }

    #[inline]
    fn goto_next_round(&mut self) {
        self.round += 1;
    }

    #[inline]
    fn goto_new_height(&mut self, new_height: usize) {
        self.height = new_height;
        self.round = 0;
        self.clean_save_info();
    }

    #[inline]
    fn clean_save_info(&mut self) {
        // clear prevote count needed when goto new height
        self.proposal = None;
        self.proposals = None;
        self.lock_status = None;
        self.vote.clear_prevote_count();
        self.authority_list = Vec::new();
    }

    fn rebroadcast_vote(&self, round: usize) {
        self.send_bft_msg(BftMsg::Vote(Vote {
            vote_type: VoteType::Prevote,
            height: self.height - 1,
            round: round,
            proposal: self.last_commit_proposal.clone().unwrap(),
            voter: self.params.clone().address,
        }));

        self.send_bft_msg(BftMsg::Vote(Vote {
            vote_type: VoteType::Precommit,
            height: self.height - 1,
            round: round,
            proposal: self.last_commit_proposal.clone().unwrap(),
            voter: self.params.clone().address,
        }));
    }

    fn determine_proposer(&self) -> bool {
        let count = if self.authority_list.len() != 0 {
            self.authority_list.len()
        } else {
            warn!("The authority list is empty!");
            return false;
        };
        let nonce = self.height + self.round;

        if self.params.address == *(self.authority_list.get(nonce % count).unwrap()) {
            return true;
        }

        let timer_duration = self.params.timer.get_propose();
        let _ = self.set_timer(timer_duration, Step::ProposeWait);
        false
    }

    fn try_broadcast_proposal(&self) -> bool {
        if self.lock_status.is_none() && self.proposals.is_none() {
            warn!("The lock status and proposals are both none!");
            self.set_timer(self.params.timer.get_propose(), Step::ProposeWait);
            return false;
        }
        let msg = if self.lock_status.is_some() {
            // if is locked, boradcast the lock proposal
            BftMsg::Proposal(Proposal {
                height: self.height,
                round: self.round,
                content: self.lock_status.clone().unwrap().proposal,
                lock_round: Some(self.lock_status.clone().unwrap().round),
                lock_votes: Some(self.lock_status.clone().unwrap().votes),
                proposer: self.params.address.clone(),
            })
        } else {
            // if is not locked, broadcast the cached proposal
            BftMsg::Proposal(Proposal {
                height: self.height,
                round: self.round,
                content: self.proposals.clone().unwrap(),
                lock_round: None,
                lock_votes: None,
                proposer: self.params.address.clone(),
            })
        };
        self.send_bft_msg(msg);
        true
    }

    fn handle_proposal(&self, proposal: Proposal) -> Option<Proposal> {
        if proposal.height == self.height - 1 && Some(proposal.round) >= self.last_commit_round {
            self.rebroadcast_vote(proposal.round);
            return None;
        } else if proposal.height != self.height || proposal.round < self.round {
            warn!("The proposal is unexpected");
            return None;
        } else {
            return Some(proposal);
        }
    }

    fn save_proposal(&mut self, proposal: Proposal) {
        if proposal.lock_round.is_some()
            && proposal.lock_votes.is_some()
            && (self.lock_status.is_none()
                || self.lock_status.clone().unwrap().round <= proposal.lock_round.unwrap())
        {
            // receive a proposal with a later PoLC
            self.round = proposal.round;
            self.proposal = Some(proposal.content.clone());
            self.lock_status = Some(LockStatus {
                proposal: proposal.content,
                round: proposal.lock_round.unwrap(),
                votes: proposal.lock_votes.unwrap(),
            });
        } else if proposal.lock_votes.is_none()
            && self.lock_status.is_none()
            && proposal.round == self.round
        {
            // receive a proposal without PoLC
            self.proposal = Some(proposal.content);
        } else if proposal.round != self.round {
            // receive a proposal of higher round
            self.proposals = Some(proposal.content);
        } else {
            warn!("The proposal is problematic!");
        }
    }

    fn broadcast_prevote(&self) {
        let prevote = if let Some(lock_proposal) = self.lock_status.clone() {
            lock_proposal.proposal
        } else if let Some(proposal) = self.proposal.clone() {
            proposal
        } else {
            Vec::new()
        };

        let msg = BftMsg::Vote(Vote {
            vote_type: VoteType::Prevote,
            height: self.height,
            round: self.round,
            proposal: prevote,
            voter: self.params.address.clone(),
        });

        self.send_bft_msg(msg);
        // self.set_timer(
        //     self.params.timer.get_prevote() * TIMEOUT_RETRANSE_MULTIPLE,
        //     Step::Prevote,
        // );
    }

    fn try_save_vote(&mut self, vote: Vote) -> bool {
        if vote.height == self.height - 1 && Some(vote.round) >= self.last_commit_round {
            self.rebroadcast_vote(vote.round);
            return false;
        } else if vote.round >= self.round && self.vote.add(
            vote.height,
            vote.round,
            vote.vote_type,
            vote.voter,
            vote.proposal,
        ) {
            return true;
        }
        false
    }

    fn check_prevote(&mut self) -> bool {
        let mut flag = false;
        for (round, prevote_count) in self.vote.prevote_count.iter() {
            if self.cal_above_threshold(*prevote_count) {
                flag = true;
                if *round > self.round {
                    self.round = *round;
                }
            }
        }
        if !flag {
            return false;
        }

        if let Some(prevote_set) = self
            .vote
            .get_voteset(self.height, self.round, VoteType::Prevote)
        {
            let mut tv = if self.cal_all_vote(prevote_set.count) {
                Duration::new(0, 0)
            } else {
                self.params.timer.get_prevote()
            };

            for (hash, count) in &prevote_set.votes_by_proposal {
                if self.cal_above_threshold(*count) {
                    if self.lock_status.is_some()
                        && self.lock_status.clone().unwrap().round < self.round
                    {
                        if hash.is_empty() {
                            // receive +2/3 prevote for nil, clean lock info
                            self.proposal = None;
                            self.lock_status = None;
                            tv = Duration::new(0, 0);
                        } else {
                            // receive a new PoLC, update lock info
                            self.proposal = Some(hash.clone());
                            self.lock_status = Some(LockStatus {
                                proposal: hash.clone(),
                                round: self.round,
                                votes: prevote_set.abstract_polc(
                                    self.height,
                                    self.round,
                                    VoteType::Prevote,
                                    hash.clone(),
                                ),
                            });
                            tv = Duration::new(0, 0);
                        }
                    }
                    break;
                }
            }
            if self.step == Step::Prevote {
                self.set_timer(tv, Step::PrevoteWait);
            }
            return true;
        }
        false
    }

    fn broadcast_precommit(&self) {
        let precommit = if let Some(lock_proposal) = self.lock_status.clone() {
            lock_proposal.proposal
        } else if let Some(proposal) = self.proposal.clone() {
            proposal
        } else {
            Vec::new()
        };

        let msg = BftMsg::Vote(Vote {
            vote_type: VoteType::Precommit,
            height: self.height,
            round: self.round,
            proposal: precommit,
            voter: self.params.address.clone(),
        });

        self.send_bft_msg(msg);
        // self.set_timer(
        //     self.params.timer.get_precommit() * TIMEOUT_RETRANSE_MULTIPLE,
        //     Step::Precommit,
        // );
    }

    fn check_precommit(&mut self) -> bool {
        if let Some(precommit_set) =
            self.vote
                .get_voteset(self.height, self.round, VoteType::Precommit)
        {
            let mut tv = if self.cal_all_vote(precommit_set.count) {
                Duration::new(0, 0)
            } else {
                self.params.timer.get_precommit()
            };

            if !self.cal_above_threshold(precommit_set.count) {
                return false;
            }

            for (hash, count) in &precommit_set.votes_by_proposal {
                if self.cal_above_threshold(*count) {
                    if hash.is_empty() {
                        // if get +2/3 precommits to nil, goto new round directly
                        self.proposal = None;
                        self.lock_status = None;
                        self.goto_next_round();
                        self.new_round_start();
                        return false;
                    } else {
                        self.proposal = Some(hash.clone());
                        self.lock_status = Some(LockStatus {
                            proposal: hash.clone(),
                            round: self.round,
                            votes: precommit_set.abstract_polc(
                                self.height,
                                self.round,
                                VoteType::Precommit,
                                hash.clone(),
                            ),
                        });
                    }
                    break;
                }
            }
            if self.step == Step::Precommit {
                self.set_timer(tv, Step::PrecommitWait);
            }
            return true;
        }
        false
    }

    fn proc_commit(&mut self) -> bool {
        if let Some(result) = self.lock_status.clone() {
            self.send_bft_msg(BftMsg::Commit(Commit {
                height: self.height,
                proposal: result.clone().proposal,
                lock_votes: self.lock_status.clone().unwrap().votes,
            }));

            self.last_commit_round = Some(self.round);
            self.last_commit_proposal = Some(result.proposal);
            self.set_timer(self.params.timer.get_commit(), Step::CommitWait);
            return true;
        }
        false
    }

    fn try_handle_status(&mut self, rich_status: RichStatus) -> bool {
        if rich_status.height >= self.height {
            // goto new height directly and update authorty list
            self.goto_new_height(rich_status.height + 1);
            self.authority_list = rich_status.authority_list;
            if let Some(interval) = rich_status.interval {
                // update the bft interval
                self.params.timer.set_total_duration(interval);
            }
            return true;
        }
        false
    }

    fn try_handle_feed(&mut self, feed: Feed) -> bool {
        if feed.height > self.height {
            self.goto_new_height(feed.height);
            self.proposals = Some(feed.proposal);
            return true;
        } else if feed.height == self.height {
            self.proposals = Some(feed.proposal);
            return false;
        } else {
            false
        }
    }

    fn new_round_start(&mut self) {
        if self.determine_proposer() {
            if self.try_broadcast_proposal() {
                self.broadcast_prevote();
                self.change_to_step(Step::Prevote);
            } else {
                self.change_to_step(Step::ProposeWait);
            }
        } else {
            self.change_to_step(Step::ProposeWait);
        }
    }

    fn process(&mut self, bft_msg: BftMsg) {
        match bft_msg {
            BftMsg::Proposal(proposal) => {
                if self.step <= Step::ProposeWait {
                    if let Some(prop) = self.handle_proposal(proposal) {
                        self.save_proposal(prop);
                        if self.step == Step::ProposeWait {
                            self.change_to_step(Step::Prevote);
                            self.broadcast_prevote();
                            if self.check_prevote() {
                                self.change_to_step(Step::PrecommitWait);
                            }
                        }
                    }
                }
            }
            BftMsg::Vote(vote) => {
                if vote.vote_type == VoteType::Prevote {
                    if self.step < Step::Prevote {
                        let _ = self.try_save_vote(vote.clone());
                    }
                    if self.step == Step::Prevote || self.step == Step::PrevoteWait {
                        let _ = self.try_save_vote(vote);
                        if self.check_prevote() {
                            self.change_to_step(Step::PrevoteWait);
                        }
                    }
                } else if vote.vote_type == VoteType::Precommit {
                    if self.step < Step::Precommit {
                        let _ = self.try_save_vote(vote.clone());
                    }
                    if self.step == Step::Precommit || self.step == Step::Precommit {
                        let _ = self.try_save_vote(vote);
                        if self.check_precommit() {
                            self.change_to_step(Step::PrevoteWait);
                        }
                    }
                } else {
                    error!("Invalid vote type!");
                }
            }
            BftMsg::Feed(feed) => {
                if self.try_handle_feed(feed) {
                    self.new_round_start();
                }
            }
            BftMsg::RichStatus(rich_status) => {
                if self.try_handle_status(rich_status) {
                    self.new_round_start();
                }
            }
            _ => error!("Invalid Message!"),
        }
    }

    fn timeout_process(&mut self, tminfo: &TimeoutInfo) {
        if tminfo.height < self.height {
            return;
        }
        if tminfo.height == self.height && tminfo.round < self.round {
            return;
        }
        if tminfo.height == self.height && tminfo.round == self.round && tminfo.step != self.step {
            return;
        }
    }
}
