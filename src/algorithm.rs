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
use voteset::{VoteCollector, VoteSet};

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::thread;
use std::time::{Duration, Instant};

use super::*;

const INIT_HEIGHT: usize = 1;
const INIT_ROUND: usize = 0;
const PROPOSAL_TIMES_COEF: usize = 10;
const PRECOMMIT_BELOW_TWO_THIRD: i8 = 0;
const PRECOMMIT_ON_NOTHING: i8 = 1;
const PRECOMMIT_ON_NIL: i8 = 2;
const PRECOMMIT_ON_PROPOSAL: i8 = 3;
const TIMEOUT_RETRANSE_MULTIPLE: u32 = 15;

/// BFT step
#[derive(Serialize, Deserialize, Debug, PartialEq, PartialOrd, Eq, Clone, Copy, Hash)]
pub enum Step {
    /// A step to determine proposer and proposer publish a proposal.
    Propose,
    /// A step to wait for proposal or feed.
    ProposeWait,
    /// A step to transmit prevote and check prevote count.
    Prevote,
    /// A step to wait for more prevote if none of them reach 2/3.
    PrevoteWait,
    /// A step to transmit precommit and check precommit count.
    Precommit,
    /// A step to wait for more prevote if none of them reach 2/3.
    PrecommitWait,
    /// A step to do commit.
    Commit,
    /// A step to wait for rich status.
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

/// BFT state message.
pub struct Bft {
    msg_sender: Sender<BftMsg>,
    msg_receiver: Receiver<BftMsg>,
    timer_seter: Sender<TimeoutInfo>,
    timer_notity: Receiver<TimeoutInfo>,

    height: usize,
    round: usize,
    step: Step,
    feed: Option<Feed>, // feed means the latest proposal given by auth at this height
    proposal: Option<Target>,
    votes: VoteCollector,
    lock_status: Option<LockStatus>,
    // wal_log: Wal,
    last_commit_round: Option<usize>,
    last_commit_proposal: Option<Target>,
    authority_list: Vec<Address>,
    htime: Instant,
    params: BftParams,
}

impl Bft {
    /// A function to start a BFT state machine.
    pub fn start(s: Sender<BftMsg>, r: Receiver<BftMsg>, local_address: Address) {
        // define message channel and timeout channel
        let (bft2timer, timer4bft) = unbounded();
        let (timer2bft, bft4timer) = unbounded();

        // start timer module.
        let timer_thread = thread::spawn(move || {
            let timer = WaitTimer::new(timer2bft, timer4bft);
            timer.start();
        });

        // start main loop module.
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
            feed: None,
            proposal: None,
            votes: VoteCollector::new(),
            lock_status: None,
            last_commit_round: None,
            last_commit_proposal: None,
            authority_list: Vec::new(),
            htime: Instant::now(),
            params: BftParams::new(local_address),
        }
    }

    #[inline]
    fn set_timer(&self, duration: Duration, step: Step) {
        self.timer_seter
            .send(TimeoutInfo {
                timeval: Instant::now() + duration,
                height: self.height,
                round: self.round,
                step,
            })
            .unwrap();
    }

    #[inline]
    fn send_bft_msg(&self, msg: BftMsg) {
        self.msg_sender.send(msg).unwrap();
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
        trace!("Goto next round {:?}", self.round + 1);
        self.round += 1;
    }

    #[inline]
    fn goto_new_height(&mut self, new_height: usize) {
        self.clean_save_info();
        self.height = new_height;
        self.round = 0;
        self.htime = Instant::now();
    }

    #[inline]
    fn clean_save_info(&mut self) {
        // clear prevote count needed when goto new height
        self.proposal = None;
        self.lock_status = None;
        self.votes.clear_prevote_count();
        self.authority_list = Vec::new();
    }

    fn retransmit_vote(&self, round: usize) {
        info!(
            "Some nodes are at low height, retransmit votes of height {:?}, round {:?}",
            self.height - 1,
            round
        );

        debug!(
            "Retransmit votes to proposal {:?}",
            self.last_commit_proposal.clone().unwrap()
        );

        self.send_bft_msg(BftMsg::Vote(Vote {
            vote_type: Step::Prevote,
            height: self.height - 1,
            round,
            proposal: self.last_commit_proposal.clone().unwrap(),
            voter: self.params.clone().address,
        }));

        self.send_bft_msg(BftMsg::Vote(Vote {
            vote_type: Step::Precommit,
            height: self.height - 1,
            round,
            proposal: self.last_commit_proposal.clone().unwrap(),
            voter: self.params.clone().address,
        }));
    }

    fn determine_proposer(&self) -> bool {
        let count = if !self.authority_list.is_empty() {
            self.authority_list.len()
        } else {
            error!("The Authority List is Empty!");
            return false;
        };

        let nonce = self.height + self.round;
        if self.params.address == self.authority_list[nonce % count] {
            info!(
                "Become proposer at height {:?}, round {:?}",
                self.height, self.round
            );
            return true;
        }

        // if is not proposer, goto step proposewait
        let coef = if self.round > PROPOSAL_TIMES_COEF {
            PROPOSAL_TIMES_COEF
        } else {
            self.round
        };

        self.set_timer(
            self.params.timer.get_propose() * 2u32.pow(coef as u32),
            Step::ProposeWait,
        );
        false
    }

    fn try_transmit_proposal(&mut self) -> bool {
        if self.lock_status.is_none()
            && (self.feed.is_none() || self.feed.clone().unwrap().height != self.height)
        {
            // if a proposer find there is no proposal nor lock, goto step proposewait
            warn!("The lock status is none and feed is mismatched!");
            let coef = if self.round > PROPOSAL_TIMES_COEF {
                PROPOSAL_TIMES_COEF
            } else {
                self.round
            };

            self.set_timer(
                self.params.timer.get_propose() * 2u32.pow(coef as u32),
                Step::ProposeWait,
            );
            return false;
        }

        let msg = if self.lock_status.is_some() {
            // if is locked, boradcast the lock proposal
            trace!(
                "Proposal at height {:?}, round {:?}, is {:?}",
                self.height,
                self.round,
                self.lock_status.clone().unwrap().proposal
            );

            BftMsg::Proposal(Proposal {
                height: self.height,
                round: self.round,
                content: self.lock_status.clone().unwrap().proposal,
                lock_round: Some(self.lock_status.clone().unwrap().round),
                lock_votes: Some(self.lock_status.clone().unwrap().votes),
                proposer: self.params.address.clone(),
            })
        } else {
            // if is not locked, transmit the cached proposal
            self.proposal = Some(self.feed.clone().unwrap().proposal);
            trace!(
                "Proposal at height {:?}, round {:?}, is {:?}",
                self.height,
                self.round,
                self.proposal.clone().unwrap()
            );

            BftMsg::Proposal(Proposal {
                height: self.height,
                round: self.round,
                content: self.proposal.clone().unwrap(),
                lock_round: None,
                lock_votes: None,
                proposer: self.params.address.clone(),
            })
        };
        info!(
            "Transmit proposal at height {:?}, round {:?}",
            self.height, self.round
        );
        self.send_bft_msg(msg);
        true
    }

    fn handle_proposal(&self, proposal: Proposal) -> Option<Proposal> {
        if proposal.height == self.height - 1 && Some(proposal.round) >= self.last_commit_round {
            // deal with height fall behind one, round ge last commit round
            self.retransmit_vote(proposal.round);
            None
        } else if proposal.height != self.height || proposal.round != self.round {
            // bft-rs lib only handle the proposals with same round, the proposals of
            // higher round should be saved outside
            warn!("Receive mismatched proposal!");
            warn!("The proposal height is {:?}, round is {:?}, self height is {:?}, round is {:?}, the proposal is {:?} !", 
                proposal.height, proposal.round, self.height, self.round, proposal.content);
            None
        } else {
            Some(proposal)
        }
    }

    fn save_proposal(&mut self, proposal: Proposal) {
        trace!(
            "Receive a proposal at height {:?}, round {:?}",
            self.height,
            self.round
        );

        if proposal.lock_round.is_some()
            && (self.lock_status.is_none()
                || self.lock_status.clone().unwrap().round <= proposal.lock_round.unwrap())
        {
            // receive a proposal with a later PoLC
            debug!(
                "Receive a proposal with the PoLC that proposal is {:?}, lock round is {:?}, lock votes are {:?}",
                proposal.content,
                proposal.lock_round,
                proposal.lock_votes
            );

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
            debug!(
                "Receive a proposal without PoLC, the proposal is {:?}",
                proposal.content
            );
            self.proposal = Some(proposal.content);
        } else {
            debug!("Receive a proposal that the PoLC is earlier than mine");
            return;
        }
    }

    fn transmit_prevote(&mut self) {
        let prevote = if let Some(lock_proposal) = self.lock_status.clone() {
            lock_proposal.proposal
        } else if let Some(proposal) = self.proposal.clone() {
            proposal
        } else {
            Vec::new()
        };

        trace!(
            "Transmit prevote at height {:?}, round {:?}",
            self.height,
            self.round
        );

        let vote = Vote {
            vote_type: Step::Prevote,
            height: self.height,
            round: self.round,
            proposal: prevote.clone(),
            voter: self.params.address.clone(),
        };

        let _ = self.votes.add(vote.clone());
        let msg = BftMsg::Vote(vote);
        debug!("Prevote to {:?}", prevote);
        self.send_bft_msg(msg);
        self.set_timer(
            self.params.timer.get_prevote() * TIMEOUT_RETRANSE_MULTIPLE,
            Step::Prevote,
        );
    }

    fn try_save_vote(&mut self, vote: Vote) -> bool {
        if vote.height == self.height - 1 && Some(vote.round) >= self.last_commit_round {
            // deal with height fall behind one, round ge last commit round
            self.retransmit_vote(vote.round);
            return false;
        } else if vote.height == self.height && self.round != 0 && vote.round == self.round - 1 {
            // deal with equal height, round fall behind
            info!("Some nodes fall behind, send nil vote to help them pursue");
            self.send_bft_msg(BftMsg::Vote(Vote {
                vote_type: Step::Precommit,
                height: vote.height,
                round: vote.round,
                proposal: Vec::new(),
                voter: self.params.clone().address,
            }));
            return false;
        } else if vote.height == self.height && vote.round >= self.round && self.votes.add(vote) {
            trace!(
                "Receive a vote at height {:?}, round {:?}",
                self.height,
                self.round
            );
            return true;
        }
        false
    }

    fn check_prevote(&mut self) -> bool {
        let mut flag = false;
        for (round, prevote_count) in self.votes.prevote_count.iter() {
            if self.cal_above_threshold(*prevote_count) && *round >= self.round {
                flag = true;
                self.round = *round;
            }
        }
        if !flag {
            return false;
        }

        info!(
            "Receive over 2/3 prevote at height {:?}, round {:?}",
            self.height, self.round
        );

        if let Some(prevote_set) = self
            .votes
            .get_voteset(self.height, self.round, Step::Prevote)
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
                            // receive +2/3 prevote to nil, clean lock info
                            trace!("Receive over 2/3 prevote to nil");
                            self.clean_polc();
                        } else {
                            // receive a newer PoLC, update lock info
                            self.set_polc(&hash, &prevote_set, Step::Prevote);
                        }
                    }
                    if self.lock_status.is_none() {
                        if !hash.is_empty() {
                            // receive a PoLC, lock the proposal
                            self.set_polc(&hash, &prevote_set, Step::Prevote);
                        }
                    }
                    tv = Duration::new(0, 0);
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

    fn transmit_precommit(&mut self) {
        let precommit = if let Some(lock_proposal) = self.lock_status.clone() {
            lock_proposal.proposal
        } else if let Some(proposal) = self.proposal.clone() {
            proposal
        } else {
            Vec::new()
        };

        trace!(
            "Transmit precommit at height {:?}, round{:?}",
            self.height,
            self.round
        );

        let vote = Vote {
            vote_type: Step::Precommit,
            height: self.height,
            round: self.round,
            proposal: precommit.clone(),
            voter: self.params.address.clone(),
        };

        let _ = self.votes.add(vote.clone());
        let msg = BftMsg::Vote(vote);
        debug!("Precommit proposal is {:?}", precommit);
        self.send_bft_msg(msg);
        self.set_timer(
            self.params.timer.get_precommit() * TIMEOUT_RETRANSE_MULTIPLE,
            Step::Precommit,
        );
    }

    fn check_precommit(&mut self) -> i8 {
        if let Some(precommit_set) =
            self.votes
                .get_voteset(self.height, self.round, Step::Precommit)
        {
            let mut tv = if self.cal_all_vote(precommit_set.count) {
                Duration::new(0, 0)
            } else {
                self.params.timer.get_precommit()
            };

            if !self.cal_above_threshold(precommit_set.count) {
                return PRECOMMIT_BELOW_TWO_THIRD;
            }

            info!(
                "Receive over 2/3 precommit at height {:?}, round {:?}",
                self.height, self.round
            );

            for (hash, count) in &precommit_set.votes_by_proposal {
                if self.cal_above_threshold(*count) {
                    if hash.is_empty() {
                        info!("Reach nil consensus, goto next round {:?}", self.round + 1);
                        return PRECOMMIT_ON_NIL;
                    } else {
                        self.set_polc(&hash, &precommit_set, Step::Precommit);
                        return PRECOMMIT_ON_PROPOSAL;
                    }
                }
            }
            if self.step == Step::Precommit {
                self.set_timer(tv, Step::PrecommitWait);
            }
            // return PRECOMMIT_ON_NOTHING;
        }
        PRECOMMIT_ON_NOTHING
    }

    fn proc_commit(&mut self) -> bool {
        if let Some(result) = self.lock_status.clone() {
            self.send_bft_msg(BftMsg::Commit(Commit {
                height: self.height,
                proposal: result.clone().proposal,
                lock_votes: self.lock_status.clone().unwrap().votes,
            }));

            info!(
                "Commit {:?} at height {:?}, consensus time {:?}.",
                result.clone().proposal,
                self.height,
                Instant::now() - self.htime
            );

            self.last_commit_round = Some(self.round);
            self.last_commit_proposal = Some(result.proposal);
            return true;
        }
        false
    }

    fn set_polc(&mut self, hash: &Target, voteset: &VoteSet, vote_type: Step) {
        self.proposal = Some(hash.to_owned());
        self.lock_status = Some(LockStatus {
            proposal: hash.to_owned(),
            round: self.round,
            votes: voteset.abstract_polc(self.height, self.round, vote_type, &hash),
        });

        info!(
            "Get PoLC at height {:?}, round {:?}, on proposal {:?}",
            self.height,
            self.round,
            hash.to_owned()
        );
    }

    fn clean_polc(&mut self) {
        self.proposal = None;
        self.lock_status = None;
        trace!(
            "Clean PoLC at height {:?}, round {:?}",
            self.height,
            self.round
        );
    }

    fn try_handle_status(&mut self, rich_status: Status) -> bool {
        // receive a rich status that height ge self.height is the only way to go to new height
        if rich_status.height >= self.height {
            // goto new height directly and update authorty list
            self.goto_new_height(rich_status.height + 1);
            self.authority_list = rich_status.authority_list;
            if let Some(interval) = rich_status.interval {
                // update the bft interval
                self.params.timer.set_total_duration(interval);
            }

            info!(
                "Receive rich status, goto new height {:?}",
                rich_status.height + 1
            );
            return true;
        }
        false
    }

    fn try_handle_feed(&mut self, feed: Feed) -> bool {
        if feed.height >= self.height {
            self.feed = Some(feed);
            info!(
                "Receive feed of height {:?}",
                self.feed.clone().unwrap().height
            );
            true
        } else {
            false
        }
    }

    fn new_round_start(&mut self) {
        info!("Start height {:?}, round{:?}", self.height, self.round);
        if self.determine_proposer() {
            if self.try_transmit_proposal() {
                self.transmit_prevote();
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
                            self.transmit_prevote();
                            if self.check_prevote() {
                                self.change_to_step(Step::PrevoteWait);
                            }
                        }
                    }
                }
            }
            BftMsg::Vote(vote) => {
                if vote.vote_type == Step::Prevote {
                    if self.step <= Step::PrevoteWait {
                        let _ = self.try_save_vote(vote);
                        if self.step >= Step::Prevote && self.check_prevote() {
                            self.change_to_step(Step::PrevoteWait);
                        }
                    }
                } else if vote.vote_type == Step::Precommit {
                    if self.step < Step::Precommit {
                        let _ = self.try_save_vote(vote.clone());
                    }
                    if self.step == Step::Precommit || self.step == Step::PrecommitWait {
                        let _ = self.try_save_vote(vote);
                        let precommit_result = self.check_precommit();
                        if precommit_result == PRECOMMIT_ON_NOTHING {
                            // only receive +2/3 precommits might lead BFT to PrecommitWait
                            self.change_to_step(Step::PrecommitWait);
                        }
                        if precommit_result == PRECOMMIT_ON_NIL {
                            // receive +2/3 on nil, goto next round directly
                            if self.lock_status.is_none() {
                                self.proposal = None;
                            }
                            self.goto_next_round();
                            self.new_round_start();
                        }
                        if precommit_result == PRECOMMIT_ON_PROPOSAL {
                            // receive +2/3 on a proposal, try to commit
                            self.change_to_step(Step::Commit);
                            self.proc_commit();
                            self.change_to_step(Step::CommitWait);
                        }
                    }
                } else {
                    error!("Invalid Vote Type!");
                }
            }
            BftMsg::Feed(feed) => {
                if self.try_handle_feed(feed) && self.step == Step::ProposeWait {
                    self.new_round_start();
                }
            }
            BftMsg::Status(rich_status) => {
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

        match tminfo.step {
            Step::ProposeWait => {
                self.change_to_step(Step::Prevote);
                self.transmit_prevote();
                if self.check_prevote() {
                    self.change_to_step(Step::PrevoteWait);
                }
            }
            Step::Prevote => {
                self.transmit_prevote();
            }
            Step::PrevoteWait => {
                self.change_to_step(Step::Precommit);
                self.transmit_precommit();
                let precommit_result = self.check_precommit();
                if precommit_result == PRECOMMIT_ON_NOTHING {
                    self.change_to_step(Step::PrecommitWait);
                }
                if precommit_result == PRECOMMIT_ON_NIL {
                    if self.lock_status.is_none() {
                        self.proposal = None;
                    }
                    self.goto_next_round();
                    self.new_round_start();
                }
                if precommit_result == PRECOMMIT_ON_PROPOSAL {
                    self.change_to_step(Step::Commit);
                    self.proc_commit();
                    self.change_to_step(Step::CommitWait);
                }
            }
            Step::Precommit => {
                self.transmit_prevote();
                self.transmit_precommit();
            }
            Step::PrecommitWait => {
                // receive +2/3 precommits however no proposal reach +2/3
                if self.lock_status.is_none() {
                    self.proposal = None;
                }
                self.goto_next_round();
                self.new_round_start();
            }
            _ => error!("Invalid Timeout Info!"),
        }
    }
}
