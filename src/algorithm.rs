use crate::*;
use crate::{
    params::BftParams,
    timer::{TimeoutInfo, WaitTimer},
    voteset::{VoteCollector, VoteSet},
};

use std::collections::HashMap;
use std::thread;
use std::time::{Duration, Instant};

use crossbeam::crossbeam_channel::{unbounded, Receiver, RecvError, Sender};

pub(crate) const INIT_HEIGHT: u64 = 0;
const INIT_ROUND: u64 = 0;
const PROPOSAL_TIMES_COEF: u64 = 10;
const PRECOMMIT_BELOW_TWO_THIRDS: i8 = 0;
const PRECOMMIT_ON_NOTHING: i8 = 1;
const PRECOMMIT_ON_NIL: i8 = 2;
const PRECOMMIT_ON_PROPOSAL: i8 = 3;
const TIMEOUT_RETRANSE_COEF: u32 = 15;
const TIMEOUT_LOW_HEIGHT_MESSAGE_COEF: u32 = 300;
const TIMEOUT_LOW_ROUND_MESSAGE_COEF: u32 = 300;

#[cfg(feature = "verify_req")]
const VERIFY_AWAIT_COEF: u32 = 50;

#[cfg(feature = "verify_req")]
#[derive(Clone, Eq, PartialEq)]
enum VerifyResult {
    Approved,
    Failed,
    Undetermined,
}

/// BFT step
#[derive(Serialize, Deserialize, Debug, PartialEq, PartialOrd, Eq, Clone, Copy, Hash)]
pub(crate) enum Step {
    /// A step to determine proposer and proposer publish a proposal.
    Propose,
    /// A step to wait for proposal or feed.
    ProposeWait,
    /// A step to transmit prevote and check prevote count.
    Prevote,
    /// A step to wait for more prevote if none of them reach 2/3.
    PrevoteWait,
    /// A step to wait for proposal verify result.
    #[cfg(feature = "verify_req")]
    VerifyWait,
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

/// BFT state message.
pub(crate) struct Bft {
    msg_sender: Sender<BftMsg>,
    msg_receiver: Receiver<BftMsg>,
    timer_seter: Sender<TimeoutInfo>,
    timer_notity: Receiver<TimeoutInfo>,

    height: u64,
    round: u64,
    step: Step,
    feed: Option<Feed>, // feed means the latest proposal given by auth at this height
    proposal: Option<Target>,
    votes: VoteCollector,
    lock_status: Option<LockStatus>,
    last_commit_round: Option<u64>,
    last_commit_proposal: Option<Target>,
    height_filter: HashMap<Address, Instant>,
    round_filter: HashMap<Address, Instant>,
    authority_list: Vec<Address>,
    htime: Instant,
    params: BftParams,

    #[cfg(feature = "verify_req")]
    verify_result: HashMap<Target, bool>,
}

impl Bft {
    /// A function to start a BFT state machine.
    pub(crate) fn start(s: Sender<BftMsg>, r: Receiver<BftMsg>, local_address: Address) {
        // define message channel and timeout channel
        let (bft2timer, timer4bft) = unbounded();
        let (timer2bft, bft4timer) = unbounded();

        // start timer module.
        let _timer_thread = thread::Builder::new()
            .name("bft_timer".to_string())
            .spawn(move || {
                let timer = WaitTimer::new(timer2bft, timer4bft);
                timer.start();
            })
            .unwrap();

        // start main loop module.
        let mut engine = Bft::initialize(s, r, bft2timer, bft4timer, local_address);
        let _main_thread = thread::Builder::new()
            .name("main_loop".to_string())
            .spawn(move || {
                let mut process_flag = false;
                loop {
                    let mut get_timer_msg = Err(RecvError);
                    let mut get_msg = Err(RecvError);

                    select! {
                        recv(engine.timer_notity) -> msg => get_timer_msg = msg,
                        recv(engine.msg_receiver) -> msg => get_msg = msg,
                    }

                    if process_flag {
                        if let Ok(ok_timer) = get_timer_msg {
                            engine.timeout_process(&ok_timer);
                        }

                        if let Ok(ok_msg) = get_msg {
                            if ok_msg == BftMsg::Pause {
                                info!("BFT pause");
                                process_flag = false;
                            } else {
                                engine.process(ok_msg);
                            }
                        }
                    } else if let Ok(ok_msg) = get_msg {
                        if ok_msg == BftMsg::Start {
                            info!("BFT go on running");
                            process_flag = true;
                        }
                    }
                }
            })
            .unwrap();
    }

    #[cfg(not(feature = "verify_req"))]
    fn initialize(
        s: Sender<BftMsg>,
        r: Receiver<BftMsg>,
        ts: Sender<TimeoutInfo>,
        tn: Receiver<TimeoutInfo>,
        local_address: Target,
    ) -> Self {
        info!("BFT State Machine Launched.");
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
            height_filter: HashMap::new(),
            round_filter: HashMap::new(),
            params: BftParams::new(local_address),
        }
    }

    #[cfg(feature = "verify_req")]
    fn initialize(
        s: Sender<BftMsg>,
        r: Receiver<BftMsg>,
        ts: Sender<TimeoutInfo>,
        tn: Receiver<TimeoutInfo>,
        local_address: Target,
    ) -> Self {
        info!("BFT State Machine Launched.");
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
            height_filter: HashMap::new(),
            round_filter: HashMap::new(),
            params: BftParams::new(local_address),
            verify_result: HashMap::new(),
        }
    }

    #[inline]
    fn set_timer(&self, duration: Duration, step: Step) {
        trace!("Set {:?} timer for {:?}", step, duration);
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
    fn clean_filter(&mut self) {
        self.height_filter.clear();
        self.round_filter.clear();
    }

    #[inline]
    fn goto_next_round(&mut self) {
        trace!("Goto next round {:?}", self.round + 1);
        self.round_filter.clear();
        self.round += 1;
    }

    #[inline]
    fn goto_new_height(&mut self, new_height: u64) {
        self.clean_save_info();
        self.clean_filter();
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

        #[cfg(feature = "verify_req")]
        self.verify_result.clear();
    }

    fn retransmit_vote(&self, round: u64) {
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
            vote_type: VoteType::Prevote,
            height: self.height - 1,
            round,
            proposal: self.last_commit_proposal.clone().unwrap(),
            voter: self.params.clone().address,
        }));

        self.send_bft_msg(BftMsg::Vote(Vote {
            vote_type: VoteType::Precommit,
            height: self.height - 1,
            round,
            proposal: self.last_commit_proposal.clone().unwrap(),
            voter: self.params.clone().address,
        }));
    }

    fn determine_height_filter(&self, sender: Address) -> (bool, bool) {
        let mut add_flag = false;
        let mut trans_flag = false;

        if let Some(ins) = self.height_filter.get(&sender) {
            // had received retransmit message from the address
            if (Instant::now() - *ins)
                > self.params.timer.get_prevote() * TIMEOUT_LOW_HEIGHT_MESSAGE_COEF
            {
                trans_flag = true;
            }
        } else {
            // never recvive retransmit message from the address
            add_flag = true;
            trans_flag = true;
        }
        (add_flag, trans_flag)
    }

    fn determine_round_filter(&self, sender: Address) -> (bool, bool) {
        let mut add_flag = false;
        let mut trans_flag = false;

        if let Some(ins) = self.round_filter.get(&sender) {
            // had received retransmit message from the address
            if (Instant::now() - *ins)
                > self.params.timer.get_prevote() * TIMEOUT_LOW_ROUND_MESSAGE_COEF
            {
                trans_flag = true;
            }
        } else {
            // never recvive retransmit message from the address
            add_flag = true;
            trans_flag = true;
        }
        (add_flag, trans_flag)
    }

    fn is_proposer(&self) -> bool {
        let count = if !self.authority_list.is_empty() {
            self.authority_list.len()
        } else {
            error!("The Authority List is Empty!");
            return false;
        };

        let nonce = self.height + self.round;
        if self.params.address == self.authority_list[(nonce as usize) % count] {
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
            info!("The lock status is none and feed is mismatched.");
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
        if proposal.height == self.height - 1 {
            if self.last_commit_round.is_some() && proposal.round >= self.last_commit_round.unwrap()
            {
                // deal with height fall behind one, round ge last commit round
                self.retransmit_vote(proposal.round);
            }
            None
        } else if proposal.height != self.height || proposal.round < self.round {
            // bft-rs lib only handle the proposals with same round, the proposals
            // with higher round should be saved outside
            warn!(
                "Receive mismatched proposal! \nThe proposal height is {:?}, \
                 round is {:?}, self height is {:?}, round is {:?}, the proposal is {:?} !",
                proposal.height, proposal.round, self.height, self.round, proposal.content
            );
            None
        } else {
            Some(proposal)
        }
    }

    fn set_proposal(&mut self, proposal: Proposal) {
        info!(
            "Receive a proposal at height {:?}, round {:?}, from {:?}",
            self.height, proposal.round, proposal.proposer
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

            if self.round < proposal.round {
                self.round_filter.clear();
                self.round = proposal.round;
            }

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
            vote_type: VoteType::Prevote,
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
            self.params.timer.get_prevote() * TIMEOUT_RETRANSE_COEF,
            Step::Prevote,
        );
    }

    fn try_save_vote(&mut self, vote: Vote) -> bool {
        info!(
            "Receive a {:?} vote of height {:?}, round {:?}, to {:?}, from {:?}",
            vote.vote_type, vote.height, vote.round, vote.proposal, vote.voter
        );

        if vote.height == self.height - 1 {
            if self.last_commit_round.is_some() && vote.round >= self.last_commit_round.unwrap() {
                // deal with height fall behind one, round ge last commit round
                let sender = vote.voter.clone();
                let (add_flag, trans_flag) = self.determine_height_filter(sender.clone());

                if add_flag {
                    self.height_filter
                        .entry(sender)
                        .or_insert_with(Instant::now);
                }
                if trans_flag {
                    self.retransmit_vote(vote.round);
                }
            }
            return false;
        } else if vote.height == self.height && self.round != 0 && vote.round == self.round - 1 {
            // deal with equal height, round fall behind
            let sender = vote.voter.clone();
            let (add_flag, trans_flag) = self.determine_round_filter(sender.clone());

            if add_flag {
                self.round_filter.entry(sender).or_insert_with(Instant::now);
            }
            if trans_flag {
                info!("Some nodes fall behind, send nil vote to help them pursue");
                self.send_bft_msg(BftMsg::Vote(Vote {
                    vote_type: VoteType::Precommit,
                    height: vote.height,
                    round: vote.round,
                    proposal: Vec::new(),
                    voter: self.params.clone().address,
                }));
            }
            return false;
        } else if vote.height == self.height
            && vote.round >= self.round
            && self.votes.add(vote.clone())
        {
            trace!("Add the vote successfully");
            return true;
        }
        trace!("Receive a saved vote");
        false
    }

    fn check_prevote_count(&mut self) -> bool {
        let mut flag = false;
        for (round, prevote_count) in self.votes.prevote_count.iter() {
            if self.cal_above_threshold(*prevote_count) && *round >= self.round {
                flag = true;
                if self.round < *round {
                    self.round_filter.clear();
                    self.round = *round;
                }
            }
        }
        if !flag {
            return false;
        }
        info!(
            "Receive over 2/3 prevote at height {:?}, round {:?}",
            self.height, self.round
        );

        if let Some(prevote_set) =
            self.votes
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
                            // receive +2/3 prevote to nil, clean lock info
                            trace!(
                                "Receive over 2/3 prevote to nil at height {:?}, round {:?}",
                                self.height,
                                self.round
                            );
                            self.clean_polc();
                            self.proposal = None;
                        } else {
                            // receive a later PoLC, update lock info
                            self.set_polc(&hash, &prevote_set, VoteType::Prevote);
                        }
                    }
                    if self.lock_status.is_none() && !hash.is_empty() {
                        // receive a PoLC, lock the proposal
                        self.set_polc(&hash, &prevote_set, VoteType::Prevote);
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

    #[cfg(feature = "verify_req")]
    fn check_verify(&mut self) -> VerifyResult {
        if let Some(lock) = self.lock_status.clone() {
            let prop = lock.proposal;
            if self.verify_result.contains_key(&prop) {
                if *self.verify_result.get(&prop).unwrap() {
                    return VerifyResult::Approved;
                } else {
                    let prop = self.lock_status.clone().unwrap().proposal;
                    if let Some(feed) = self.feed.clone() {
                        // if feed eq proposal, clean it
                        if prop == feed.proposal {
                            self.feed = None;
                        }
                    }
                    // clean save info
                    self.clean_polc();
                    return VerifyResult::Failed;
                }
            } else {
                let tv = self.params.timer.get_prevote() * VERIFY_AWAIT_COEF;
                self.set_timer(tv, Step::VerifyWait);
                return VerifyResult::Undetermined;
            }
        }
        VerifyResult::Approved
    }

    fn transmit_precommit(&mut self) {
        let precommit = if let Some(lock_proposal) = self.lock_status.clone() {
            lock_proposal.proposal
        } else {
            self.proposal = None;
            Vec::new()
        };

        trace!(
            "Transmit precommit at height {:?}, round {:?}",
            self.height,
            self.round
        );

        let vote = Vote {
            vote_type: VoteType::Precommit,
            height: self.height,
            round: self.round,
            proposal: precommit.clone(),
            voter: self.params.address.clone(),
        };

        let _ = self.votes.add(vote.clone());
        let msg = BftMsg::Vote(vote);
        debug!("Precommit to {:?}", precommit);
        self.send_bft_msg(msg);
        self.set_timer(
            self.params.timer.get_precommit() * TIMEOUT_RETRANSE_COEF,
            Step::Precommit,
        );
    }

    fn check_precommit_count(&mut self) -> i8 {
        if let Some(precommit_set) =
            self.votes
                .get_voteset(self.height, self.round, VoteType::Precommit)
        {
            let tv = if self.cal_all_vote(precommit_set.count) {
                Duration::new(0, 0)
            } else {
                self.params.timer.get_precommit()
            };
            if !self.cal_above_threshold(precommit_set.count) {
                return PRECOMMIT_BELOW_TWO_THIRDS;
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
                        self.set_polc(&hash, &precommit_set, VoteType::Precommit);
                        return PRECOMMIT_ON_PROPOSAL;
                    }
                }
            }
            if self.step == Step::Precommit {
                self.set_timer(tv, Step::PrecommitWait);
            }
        }
        PRECOMMIT_ON_NOTHING
    }

    fn proc_commit(&mut self) {
        let result = self.lock_status.clone().expect("No lock when commit!");
        self.send_bft_msg(BftMsg::Commit(Commit {
            height: self.height,
            round: self.round,
            proposal: result.clone().proposal,
            lock_votes: self.lock_status.clone().unwrap().votes,
            address: self.params.clone().address,
        }));

        info!(
            "Commit {:?} at height {:?}, consensus time {:?}",
            result.clone().proposal,
            self.height,
            Instant::now() - self.htime
        );

        self.last_commit_round = Some(self.round);
        self.last_commit_proposal = Some(result.proposal);
    }

    fn set_polc(&mut self, hash: &[u8], voteset: &VoteSet, vote_type: VoteType) {
        self.proposal = Some(hash.to_owned());
        self.lock_status = Some(LockStatus {
            proposal: hash.to_owned(),
            round: self.round,
            votes: voteset.extract_polc(self.height, self.round, vote_type, hash),
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
            if rich_status.height > self.height {
                // recvive higher status, clean last commit info then go to new height
                self.last_commit_proposal = None;
                self.last_commit_round = None;
            }
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

    #[cfg(feature = "verify_req")]
    fn save_verify_resp(&mut self, verify_result: VerifyResp) {
        if self.verify_result.contains_key(&verify_result.proposal) {
            if &verify_result.is_pass != self.verify_result.get(&verify_result.proposal).unwrap() {
                error!(
                    "The verify results of {:?} are different!",
                    verify_result.proposal
                );
                return;
            }
        }
        info!(
            "Receive verify result of proposal {:?}",
            verify_result.proposal.clone()
        );
        self.verify_result
            .entry(verify_result.proposal)
            .or_insert(verify_result.is_pass);
    }

    fn new_round_start(&mut self) {
        if self.step != Step::ProposeWait {
            info!("Start height {:?}, round{:?}", self.height, self.round);
        }
        if self.is_proposer() {
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
                        self.set_proposal(prop);
                        if self.step == Step::ProposeWait {
                            self.change_to_step(Step::Prevote);
                            self.transmit_prevote();
                            if self.check_prevote_count() {
                                self.change_to_step(Step::PrevoteWait);
                            }
                        }
                    }
                }
            }
            BftMsg::Vote(vote) => {
                if vote.vote_type == VoteType::Prevote {
                    if self.step <= Step::PrevoteWait {
                        let _ = self.try_save_vote(vote);
                        if self.step >= Step::Prevote && self.check_prevote_count() {
                            self.change_to_step(Step::PrevoteWait);
                        }
                    }
                } else if vote.vote_type == VoteType::Precommit {
                    if self.step < Step::Precommit {
                        let _ = self.try_save_vote(vote.clone());
                    }
                    if (self.step == Step::Precommit || self.step == Step::PrecommitWait)
                        && self.try_save_vote(vote)
                    {
                        let precommit_result = self.check_precommit_count();

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

            #[cfg(feature = "verify_req")]
            BftMsg::VerifyResp(verify_resp) => {
                self.save_verify_resp(verify_resp);
                if self.step == Step::VerifyWait {
                    // next do precommit
                    self.change_to_step(Step::Precommit);
                    if self.check_verify() == VerifyResult::Undetermined {
                        self.change_to_step(Step::VerifyWait);
                        return;
                    }
                    self.transmit_precommit();
                    let precommit_result = self.check_precommit_count();

                    if precommit_result == PRECOMMIT_ON_NOTHING {
                        // only receive +2/3 precommits might lead BFT to PrecommitWait
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
                if self.check_prevote_count() {
                    self.change_to_step(Step::PrevoteWait);
                }
            }
            Step::Prevote => {
                self.transmit_prevote();
            }
            Step::PrevoteWait => {
                // if there is no lock, clear the proposal
                if self.lock_status.is_none() {
                    self.proposal = None;
                }
                // next do precommit
                self.change_to_step(Step::Precommit);
                #[cfg(feature = "verify_req")]
                {
                    let verify_result = self.check_verify();
                    if verify_result == VerifyResult::Undetermined {
                        self.change_to_step(Step::VerifyWait);
                        return;
                    }
                }

                self.transmit_precommit();
                let precommit_result = self.check_precommit_count();

                if precommit_result == PRECOMMIT_ON_NOTHING {
                    // only receive +2/3 precommits might lead BFT to PrecommitWait
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
                // then goto next round directly
                self.goto_next_round();
                self.new_round_start();
            }

            #[cfg(feature = "verify_req")]
            Step::VerifyWait => {
                // clean fsave info
                self.clean_polc();

                // next do precommit
                self.change_to_step(Step::Precommit);
                self.transmit_precommit();
                let precommit_result = self.check_precommit_count();

                if precommit_result == PRECOMMIT_ON_NOTHING {
                    // only receive +2/3 precommits might lead BFT to PrecommitWait
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

            _ => error!("Invalid Timeout Info!"),
        }
    }
}
