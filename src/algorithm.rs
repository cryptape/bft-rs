use crate::*;
use crate::{
    collectors::{ProposalCollector, VoteCollector, VoteSet},
    error::BftError,
    objects::*,
    params::BftParams,
    random::get_proposer,
    timer::{TimeoutInfo, WaitTimer},
    wal::Wal,
};

use crossbeam::crossbeam_channel::{unbounded, Receiver, RecvError, Sender};
use crossbeam_utils::thread as cross_thread;
use std::collections::HashMap;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

pub(crate) const INIT_HEIGHT: u64 = 0;
pub(crate) const INIT_ROUND: u64 = 0;
const PROPOSAL_TIMES_COEF: u64 = 10;
const TIMEOUT_RETRANSE_COEF: u32 = 15;
const TIMEOUT_LOW_HEIGHT_MESSAGE_COEF: u32 = 20;
const TIMEOUT_LOW_ROUND_MESSAGE_COEF: u32 = 20;

#[cfg(feature = "verify_req")]
const VERIFY_AWAIT_COEF: u32 = 50;

/// BFT state message.
pub struct Bft<T: BftSupport> {
    // channel
    pub(crate) msg_sender: Sender<BftMsg>,
    pub(crate) msg_receiver: Receiver<BftMsg>,
    pub(crate) timer_seter: Sender<TimeoutInfo>,
    pub(crate) timer_notity: Receiver<TimeoutInfo>,
    // bft-core params
    pub(crate) height: u64,
    pub(crate) round: u64,
    pub(crate) step: Step,
    pub(crate) block_hash: Option<Hash>,
    pub(crate) lock_status: Option<LockStatus>,
    pub(crate) height_filter: HashMap<Address, Instant>,
    pub(crate) round_filter: HashMap<Address, Instant>,
    pub(crate) last_commit_round: Option<u64>,
    pub(crate) last_commit_block_hash: Option<Hash>,
    pub(crate) authority_manage: AuthorityManage,
    pub(crate) params: BftParams,
    pub(crate) htime: Instant,
    // caches
    pub(crate) feeds: HashMap<u64, Vec<u8>>,
    pub(crate) verify_results: HashMap<Hash, bool>,
    pub(crate) proof: Option<Proof>,
    pub(crate) proposals: ProposalCollector,
    pub(crate) votes: VoteCollector,
    pub(crate) wal_log: Wal,
    // user define
    pub(crate) function: Arc<T>,
    pub(crate) consensus_power: bool,
}

impl<T> Bft<T>
where
    T: BftSupport + 'static,
{
    /// A function to start a BFT state machine.
    pub fn start(
        s: Sender<BftMsg>,
        r: Receiver<BftMsg>,
        f: Arc<T>,
        local_address: Address,
        wal_path: &str,
    ) {
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
        let mut engine = Bft::initialize(s, r, bft2timer, bft4timer, f, local_address, wal_path);
        engine.load_wal_log();

        let _main_thread = thread::Builder::new()
            .name("main_loop".to_string())
            .spawn(move || {
                loop {
                    let mut get_timer_msg = Err(RecvError);
                    let mut get_msg = Err(RecvError);

                    select! {
                        recv(engine.timer_notity) -> msg => get_timer_msg = msg,
                        recv(engine.msg_receiver) -> msg => get_msg = msg,
                    }

                    if let Ok(ok_timer) = get_timer_msg {
                        //TODO: change the output of timeout_process to BftResult
                        engine.timeout_process(&ok_timer);
                    }

                    if let Ok(ok_msg) = get_msg {
                        if let Err(error) = engine.process(ok_msg) {
                            warn!("Bft process encounters {:?}!", error);
                        }
                    }
                }
            })
            .unwrap();
    }

    fn process(&mut self, msg: BftMsg) -> BftResult<()> {
        match msg {
            BftMsg::Proposal(encode) => {
                if self.consensus_power {
                    let signed_proposal: SignedProposal =
                        rlp::decode(&encode).or(Err(BftError::DecodeErr))?;
                    trace!("Bft receives signed_proposal {:?}", &signed_proposal);
                    self.check_and_save_proposal(&signed_proposal, &encode, true)?;

                    let proposal = signed_proposal.proposal;
                    if self.step <= Step::ProposeWait {
                        if let Some(prop) = self.handle_proposal(proposal) {
                            self.set_proposal(prop);
                            if self.step == Step::ProposeWait {
//                                self.change_to_step(Step::Prevote);
                                self.transmit_prevote();
//                                if self.check_prevote_count() {
//                                    self.change_to_step(Step::PrevoteWait);
//                                }
                            }
                        }
                    }
                }
            }

            BftMsg::Vote(encode) => {
                if self.consensus_power {
                    let signed_vote: SignedVote =
                        rlp::decode(&encode).or(Err(BftError::DecodeErr))?;
                    trace!("Bft receives signed_vote {:?}", &signed_vote);
                    self.check_and_save_vote(&signed_vote, true)?;

                    let vote = signed_vote.vote;
                    if vote.vote_type == VoteType::Prevote {
                        if self.step <= Step::PrevoteWait {
                            let _ = self.handle_vote(vote);
                            if self.step >= Step::Prevote && self.check_prevote_count() {
                                self.change_to_step(Step::PrevoteWait);
                            }
                        }
                    } else if vote.vote_type == VoteType::Precommit {
                        if self.step < Step::Precommit {
                            let _ = self.handle_vote(vote.clone());
                        }
                        if (self.step == Step::Precommit || self.step == Step::PrecommitWait)
                            && self.handle_vote(vote)
                        {
                            self.handle_precommit();
                        }
                    } else {
                        return Err(BftError::MsgTypeErr);
                    }
                }
            }

            BftMsg::Feed(feed) => {
                trace!("Bft receives feed {:?}", &feed);
                self.check_and_save_feed(feed, true)?;

                if self.step == Step::ProposeWait {
                    self.new_round_start(false);
                }
            }

            BftMsg::Status(status) => {
                trace!("Bft receives status {:?}", &status);
                self.check_and_save_status(&status, true)?;

                if self.try_handle_status(status) {
                    self.new_round_start(true);
                }
            }

            #[cfg(feature = "verify_req")]
            BftMsg::VerifyResp(verify_resp) => {
                trace!("Bft receives verify_resp {:?}", &verify_resp);
                self.check_and_save_verify_resp(&verify_resp, true)?;

                if self.step == Step::VerifyWait {
                    // next do precommit
                    self.change_to_step(Step::Precommit);
                    if self.check_verify() == VerifyResult::Undetermined {
                        self.change_to_step(Step::VerifyWait);
                    } else {
                        self.transmit_precommit();
//                        self.handle_precommit();
                    }
                }
            }

            BftMsg::Pause => self.consensus_power = false,

            BftMsg::Start => self.consensus_power = true,

            BftMsg::Clear => {
                self.clear();
            }
        }

        Ok(())
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
                    self.block_hash = None;
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
//                self.handle_precommit();
            }
            Step::Precommit => {
                self.transmit_prevote();
                self.transmit_precommit();
            }
            Step::PrecommitWait => {
                // receive +2/3 precommits however no proposal reach +2/3
                // then goto next round directly
                self.goto_next_round();
                self.new_round_start(true);
            }

            #[cfg(feature = "verify_req")]
            Step::VerifyWait => {
                // clean fsave info
                self.clean_polc();

                // next do precommit
                self.change_to_step(Step::Precommit);
                self.transmit_precommit();
//                self.handle_precommit();
            }
            _ => error!("Invalid Timeout Info!"),
        }
    }

    fn initialize(
        s: Sender<BftMsg>,
        r: Receiver<BftMsg>,
        ts: Sender<TimeoutInfo>,
        tn: Receiver<TimeoutInfo>,
        f: Arc<T>,
        local_address: Hash,
        wal_path: &str,
    ) -> Self {
        info!("Bft Address: {:?}, wal_path: {}", local_address, wal_path);
        Bft {
            msg_sender: s,
            msg_receiver: r,
            timer_seter: ts,
            timer_notity: tn,
            height: INIT_HEIGHT,
            round: INIT_ROUND,
            step: Step::default(),
            block_hash: None,
            lock_status: None,
            height_filter: HashMap::new(),
            round_filter: HashMap::new(),
            last_commit_round: None,
            last_commit_block_hash: None,
            htime: Instant::now(),
            params: BftParams::new(local_address),
            feeds: HashMap::new(),
            verify_results: HashMap::new(),
            proof: Some(Proof::default()),
            authority_manage: AuthorityManage::new(),
            proposals: ProposalCollector::new(),
            votes: VoteCollector::new(),
            wal_log: Wal::new(wal_path).unwrap(),
            function: f,
            consensus_power: false,
        }
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
                proposal.height,
                proposal.round,
                self.height,
                self.round,
                self.function.crypt_hash(&proposal.block)
            );
            None
        } else {
            Some(proposal)
        }
    }

    fn handle_vote(&mut self, vote: Vote) -> bool {
        info!(
            "Bft handles a {:?} vote of height {:?}, round {:?}, to {:?}, from {:?}",
            vote.vote_type, vote.height, vote.round, vote.block_hash, vote.voter
        );

        if vote.height == self.height - 1 {
            if self.last_commit_round.is_some() && vote.round >= self.last_commit_round.unwrap() {
                // deal with height fall behind one, round ge last commit round
                let voter = vote.voter.clone();
                let trans_flag = self.filter_height(&voter);

                if trans_flag {
                    self.height_filter.insert(voter, Instant::now());
                    self.retransmit_vote(vote.round);
                }
            }
            return false;
        } else if vote.height == self.height && self.round != 0 && vote.round == self.round - 1 {
            // deal with equal height, round fall behind
            let voter = vote.voter.clone();
            let trans_flag = self.filter_round(&voter);

            if trans_flag {
                self.round_filter.insert(voter, Instant::now());
                self.retransmit_nil_precommit(&vote);
            }
            return false;
        } else if vote.height == self.height && vote.round >= self.round {
            return true;
        }
        false
    }

    fn handle_precommit(&mut self) {
        let result = self.check_precommit_count();
        match result {
            PrecommitRes::Above => self.change_to_step(Step::PrecommitWait),
            PrecommitRes::Nil => {
                if self.lock_status.is_none() {
                    self.block_hash = None;
                }
                self.goto_next_round();
                self.new_round_start(true);
            }
            PrecommitRes::Proposal => {
                self.change_to_step(Step::Commit);
                self.handle_commit();
                self.change_to_step(Step::CommitWait);
            }
            _ => {}
        }
    }

    fn handle_commit(&mut self) {
        let lock_status = self.lock_status.clone().expect("No lock when commit!");

        let proof = self.generate_proof(lock_status.clone());
        self.proof = Some(proof.clone());

        let proposal = self
            .proposals
            .get_proposal(self.height, self.round)
            .unwrap();

        let commit = Commit {
            height: self.height,
            block: proposal.block.clone(),
            proof,
            address: proposal.proposer.clone(),
        };

        self.function.commit(commit).unwrap();

        info!(
            "Bft commits {:?} at height {:?}, consumes consensus time {:?}",
            lock_status.block_hash,
            self.height,
            Instant::now() - self.htime
        );

        self.last_commit_round = Some(self.round);
        self.last_commit_block_hash = Some(self.function.crypt_hash(&proposal.block));
    }

    fn try_handle_status(&mut self, status: Status) -> bool {
        // commit timeout since pub block to chain,so resending the block
        if self.height > 0 && status.height == self.height - 1 && self.step >= Step::Commit {
            self.handle_commit();
        }

        // receive a rich status that height ge self.height is the only way to go to new height
        if status.height >= self.height {
            if status.height > self.height {
                // recvive higher status, clean last commit info then go to new height
                self.last_commit_block_hash = None;
                self.last_commit_round = None;
            }
            // goto new height directly and update authorty list

            self.authority_manage
                .receive_authorities_list(status.height, status.authority_list.clone());
            trace!("Bft updates authority_manage {:?}", self.authority_manage);

            if self.consensus_power
                && !status
                    .authority_list
                    .iter()
                    .any(|node| node.address == self.params.address)
            {
                info!(
                    "Bft loses consensus power in height {} and stops the bft-rs process!",
                    status.height
                );
                self.consensus_power = false;
            } else if !self.consensus_power
                && status
                    .authority_list
                    .iter()
                    .any(|node| node.address == self.params.address)
            {
                info!(
                    "Bft accesses consensus power in height {} and starts the bft-rs process!",
                    status.height
                );
                self.consensus_power = true;
            }

            if let Some(interval) = status.interval {
                // update the bft interval
                self.params.timer.set_total_duration(interval);
            }

            self.goto_new_height(status.height + 1);

            self.flush_cache();

            info!(
                "Bft receives rich status, goto new height {:?}",
                status.height + 1
            );
            return true;
        }
        false
    }

    fn try_transmit_proposal(&mut self) -> bool {
        if self.lock_status.is_none()
            && (self.feeds.get(&self.height).is_none()
                || self.proof.is_none()
                || (self.proof.iter().next().unwrap().height != self.height - 1))
        {
            // if a proposer find there is no proposal nor lock, goto step proposewait
            info!(
                "Bft is not ready to transmit proposal, lock_status {:?}, feed {:?}, proof {:?}",
                self.lock_status,
                self.feeds.get(&self.height),
                self.proof
            );
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
            info!("Bft is ready to transmit locked Proposal");
            let lock_status = self.lock_status.clone().unwrap();
            let lock_round = lock_status.round;
            let lock_votes = lock_status.votes;
            //TODO: bug: the lock_proposal may be flushed out
            let lock_proposal = self
                .proposals
                .get_proposal(self.height, lock_round)
                .unwrap();

            let block = lock_proposal.block;

            let proposal = Proposal {
                height: self.height,
                round: self.round,
                block,
                proof: lock_proposal.proof,
                lock_round: Some(lock_round),
                lock_votes,
                proposer: self.params.address.clone(),
            };

            let signed_proposal = self.build_signed_proposal(&proposal);
            BftMsg::Proposal(rlp::encode(&signed_proposal))
        } else {
            // if is not locked, transmit the cached proposal
            let block = self.feeds.get(&self.height).unwrap().clone();
            let block_hash = self.function.crypt_hash(&block);
            self.block_hash = Some(block_hash.clone());
            info!("Bft is ready to transmit new Proposal");

            let proposal = Proposal {
                height: self.height,
                round: self.round,
                block,
                proof: self.proof.clone().unwrap(),
                lock_round: None,
                lock_votes: Vec::new(),
                proposer: self.params.address.clone(),
            };

            let signed_proposal = self.build_signed_proposal(&proposal);
            BftMsg::Proposal(rlp::encode(&signed_proposal))
        };
        info!(
            "Bft transmits proposal at height {:?}, round {:?}",
            self.height, self.round
        );
        self.function.transmit(msg.clone());
        self.send_bft_msg(msg);
        true
    }

    fn transmit_prevote(&mut self) {
        let block_hash = if let Some(lock_status) = self.lock_status.clone() {
            lock_status.block_hash
        } else if let Some(block_hash) = self.block_hash.clone() {
            block_hash
        } else {
            Vec::new()
        };

        info!(
            "Bft transmits prevote at height {:?}, round {:?}",
            self.height, self.round
        );

        self.change_to_step(Step::Prevote);

        let vote = Vote {
            vote_type: VoteType::Prevote,
            height: self.height,
            round: self.round,
            block_hash: block_hash.clone(),
            voter: self.params.address.clone(),
        };
        let signed_vote = self.build_signed_vote(&vote);

//        let vote_weight = self.get_vote_weight(self.height, &vote.voter);
//        let _ = self.votes.add(&signed_vote, vote_weight, self.height);

        let msg = BftMsg::Vote(rlp::encode(&signed_vote));

        self.function.transmit(msg.clone());
        self.send_bft_msg(msg);
        info!("Bft prevotes to {:?}", block_hash);

        self.set_timer(
            self.params.timer.get_prevote() * TIMEOUT_RETRANSE_COEF,
            Step::Prevote,
        );
    }

    fn transmit_precommit(&mut self) {
        let block_hash = if let Some(lock_status) = self.lock_status.clone() {
            lock_status.block_hash
        } else {
            self.block_hash = None;
            Vec::new()
        };

        info!(
            "Bft transmits precommit at height {:?}, round {:?}",
            self.height, self.round
        );

        let vote = Vote {
            vote_type: VoteType::Precommit,
            height: self.height,
            round: self.round,
            block_hash: block_hash.clone(),
            voter: self.params.address.clone(),
        };
        let signed_vote = self.build_signed_vote(&vote);

//        let vote_weight = self.get_vote_weight(self.height, &vote.voter);
//        let _ = self.votes.add(&signed_vote, vote_weight, self.height);

        let msg = BftMsg::Vote(rlp::encode(&signed_vote));
        self.function.transmit(msg.clone());
        self.send_bft_msg(msg);
        info!("Bft precommits to {:?}", block_hash);

        self.set_timer(
            self.params.timer.get_precommit() * TIMEOUT_RETRANSE_COEF,
            Step::Precommit,
        );
    }

    fn retransmit_vote(&self, round: u64) {
        info!(
            "Bft finds some nodes are at low height, retransmit votes of height {:?}, round {:?}",
            self.height - 1,
            round
        );

        info!(
            "Bft retransmits votes to proposal {:?}",
            self.last_commit_block_hash.clone().unwrap()
        );

        let prevote = Vote {
            vote_type: VoteType::Prevote,
            height: self.height - 1,
            round,
            block_hash: self.last_commit_block_hash.clone().unwrap(),
            voter: self.params.address.clone(),
        };

        let signed_prevote = self.build_signed_vote(&prevote);
        self.function
            .transmit(BftMsg::Vote(rlp::encode(&signed_prevote)));

        let precommit = Vote {
            vote_type: VoteType::Precommit,
            height: self.height - 1,
            round,
            block_hash: self.last_commit_block_hash.clone().unwrap(),
            voter: self.params.address.clone(),
        };
        let signed_precommit = self.build_signed_vote(&precommit);
        self.function
            .transmit(BftMsg::Vote(rlp::encode(&signed_precommit)));
    }

    fn retransmit_nil_precommit(&self, vote: &Vote) {
        let precommit = Vote {
            vote_type: VoteType::Precommit,
            height: vote.height,
            round: vote.round,
            block_hash: Vec::new(),
            voter: self.params.address.clone(),
        };
        let signed_precommit = self.build_signed_vote(&precommit);

        info!("Bft finds some nodes fall behind, and sends nil vote to help them pursue");
        self.function
            .transmit(BftMsg::Vote(rlp::encode(&signed_precommit)));
    }

    #[inline]
    fn change_to_step(&mut self, step: Step) {
        self.step = step;
    }

    fn new_round_start(&mut self, new_round: bool) {
        if self.step != Step::ProposeWait {
            info!("Bft starts height {:?}, round{:?}", self.height, self.round);
        }
        if self.is_proposer() {
            if new_round {
                let function = self.function.clone();
                let sender = self.msg_sender.clone();
                let height = self.height;
                cross_thread::scope(|s| {
                    s.spawn(move |_| {
                        if let Ok(block) = function.get_block(height) {
                            let feed = Feed { height, block };
                            sender.send(BftMsg::Feed(feed)).unwrap();
                        }
                    });
                })
                .unwrap();
            }

            if self.try_transmit_proposal() {
                self.transmit_prevote();
//                self.change_to_step(Step::Prevote);
            } else {
                self.change_to_step(Step::ProposeWait);
            }
        } else {
            self.change_to_step(Step::ProposeWait);
        }
    }

    #[inline]
    fn goto_new_height(&mut self, new_height: u64) {
        self.clean_save_info();
        self.clean_filter();
        let _ = self.wal_log.set_height(new_height);

        self.height = new_height;
        self.round = 0;
        self.htime = Instant::now();
    }

    #[inline]
    fn goto_next_round(&mut self) {
        info!("Bft goto next round {:?}", self.round + 1);
        self.round_filter.clear();
        self.round += 1;
    }

    fn is_proposer(&self) -> bool {
        let authorities = &self.authority_manage.authorities;
        if authorities.is_empty() {
            error!("The Authority List is Empty!");
            return false;
        }

        let nonce = self.height + self.round;
        let weight: Vec<u64> = authorities
            .iter()
            .map(|node| node.proposal_weight as u64)
            .collect();
        let proposer: &Address = &authorities
            .get(get_proposer(nonce, &weight))
            .unwrap()
            .address;
        if self.params.address == *proposer {
            info!(
                "Bft becomes proposer at height {:?}, round {:?}",
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

    fn filter_height(&self, voter: &Address) -> bool {
        let mut trans_flag = false;

        if let Some(ins) = self.height_filter.get(voter) {
            // had received retransmit message from the address
            if (Instant::now() - *ins)
                > self.params.timer.get_prevote() * TIMEOUT_LOW_HEIGHT_MESSAGE_COEF
            {
                trans_flag = true;
            }
        } else {
            trans_flag = true;
        }
        trans_flag
    }

    fn filter_round(&self, voter: &Address) -> bool {
        let mut trans_flag = false;

        if let Some(ins) = self.round_filter.get(voter) {
            // had received retransmit message from the address
            if (Instant::now() - *ins)
                > self.params.timer.get_prevote() * TIMEOUT_LOW_ROUND_MESSAGE_COEF
            {
                trans_flag = true;
            }
        } else {
            trans_flag = true;
        }
        trans_flag
    }

    fn set_proposal(&mut self, proposal: Proposal) {
        let block_hash = self.function.crypt_hash(&proposal.block);

        if proposal.lock_round.is_some()
            && (self.lock_status.is_none()
                || self.lock_status.clone().unwrap().round <= proposal.lock_round.unwrap())
        {
            // receive a proposal with a later PoLC
            info!(
                    "Bft handles a proposal with the PoLC that proposal is {:?}, lock round is {:?}, lock votes are {:?}",
                    block_hash,
                    proposal.lock_round,
                    proposal.lock_votes
                );

            if self.round < proposal.round {
                self.round_filter.clear();
                self.round = proposal.round;
            }

            self.block_hash = Some(block_hash.clone());
            self.lock_status = Some(LockStatus {
                block_hash,
                round: proposal.lock_round.unwrap(),
                votes: proposal.lock_votes,
            });
        } else if proposal.lock_round.is_none()
            && self.lock_status.is_none()
            && proposal.round == self.round
        {
            // receive a proposal without PoLC
            info!(
                "Bft handles a proposal without PoLC, the proposal is {:?}",
                block_hash
            );
            self.block_hash = Some(block_hash);
        } else {
            info!("Bft handles a proposal with an earlier PoLC");
            return;
        }
    }

    fn set_polc(&mut self, hash: &[u8], voteset: &VoteSet) {
        self.block_hash = Some(hash.to_owned());
        self.lock_status = Some(LockStatus {
            block_hash: hash.to_owned(),
            round: self.round,
            votes: voteset.extract_polc(hash),
        });

        info!(
            "Bft sets a PoLC at height {:?}, round {:?}, on proposal {:?}",
            self.height,
            self.round,
            hash.to_owned()
        );
    }

    #[inline]
    fn set_timer(&self, duration: Duration, step: Step) {
        info!("Bft sets {:?} timer for {:?}", step, duration);
        self.timer_seter
            .send(TimeoutInfo {
                timeval: Instant::now() + duration,
                height: self.height,
                round: self.round,
                step,
            })
            .unwrap();
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
            "Bft collects over 2/3 prevotes at height {:?}, round {:?}",
            self.height, self.round
        );

        if let Some(prevote_set) =
            self.votes
                .get_voteset(self.height, self.round, &VoteType::Prevote)
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
                            info!(
                                "Bft collects over 2/3 prevotes to nil at height {:?}, round {:?}",
                                self.height, self.round
                            );
                            self.clean_polc();
                            self.block_hash = None;
                        } else {
                            // receive a later PoLC, update lock info
                            self.set_polc(&hash, &prevote_set);
                        }
                    }
                    if self.lock_status.is_none() && !hash.is_empty() {
                        // receive a PoLC, lock the proposal
                        self.set_polc(&hash, &prevote_set);
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

    fn check_precommit_count(&mut self) -> PrecommitRes {
        if let Some(precommit_set) =
            self.votes
                .get_voteset(self.height, self.round, &VoteType::Precommit)
        {
            let tv = if self.cal_all_vote(precommit_set.count) {
                Duration::new(0, 0)
            } else {
                self.params.timer.get_precommit()
            };
            if !self.cal_above_threshold(precommit_set.count) {
                return PrecommitRes::Below;
            }

            info!(
                "Bft collects over 2/3 precommits at height {:?}, round {:?}",
                self.height, self.round
            );

            for (hash, count) in &precommit_set.votes_by_proposal {
                if self.cal_above_threshold(*count) {
                    if hash.is_empty() {
                        info!(
                            "Bft reaches nil consensus, goto next round {:?}",
                            self.round + 1
                        );
                        return PrecommitRes::Nil;
                    } else {
                        self.set_polc(&hash, &precommit_set);
                        return PrecommitRes::Proposal;
                    }
                }
            }
            if self.step == Step::Precommit {
                self.set_timer(tv, Step::PrecommitWait);
            }
        }
        PrecommitRes::Above
    }

    #[cfg(feature = "verify_req")]
    fn check_verify(&mut self) -> VerifyResult {
        if let Some(lock_status) = self.lock_status.clone() {
            let block_hash = lock_status.block_hash;
            if self.verify_results.contains_key(&block_hash) {
                if *self.verify_results.get(&block_hash).unwrap() {
                    return VerifyResult::Approved;
                } else {
                    let feed = self.feeds.get(&self.height);
                    if feed.is_some() && self.function.crypt_hash(feed.unwrap()) == block_hash {
                        self.feeds.remove(&self.height);
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

    #[inline]
    fn cal_all_vote(&self, count: u64) -> bool {
        let weight: Vec<u64> = self
            .authority_manage
            .authorities
            .iter()
            .map(|node| node.vote_weight as u64)
            .collect();
        let weight_sum: u64 = weight.iter().sum();
        count == weight_sum
    }

    #[inline]
    fn cal_above_threshold(&self, count: u64) -> bool {
        let weight: Vec<u64> = self
            .authority_manage
            .authorities
            .iter()
            .map(|node| node.vote_weight as u64)
            .collect();
        let weight_sum: u64 = weight.iter().sum();
        count * 3 > weight_sum * 2
    }

    fn clean_polc(&mut self) {
        self.block_hash = None;
        self.lock_status = None;
        info!(
            "Bft cleans PoLC at height {:?}, round {:?}",
            self.height, self.round
        );
    }

    #[inline]
    fn clean_save_info(&mut self) {
        // clear prevote count needed when goto new height
        self.block_hash = None;
        self.lock_status = None;
        self.votes.clear_prevote_count();
        self.clean_feeds();

        #[cfg(feature = "verify_req")]
        self.verify_results.clear();
    }

    #[inline]
    fn clean_feeds(&mut self) {
        let height = self.height;
        self.feeds.retain(|&hi, _| hi >= height);
    }

    #[inline]
    fn clean_filter(&mut self) {
        self.height_filter.clear();
        self.round_filter.clear();
    }
}
