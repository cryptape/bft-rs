use crate::*;
use crate::{
    params::BftParams,
    error::BftError,
    random::get_proposer,
    timer::{TimeoutInfo, WaitTimer},
    voteset::{ProposalCollector, VoteCollector, RoundCollector, VoteSet},
    wal::Wal,
};

use crossbeam::crossbeam_channel::{unbounded, Receiver, RecvError, Sender};
use std::collections::HashMap;
use std::fs;
use std::thread;
use std::time::{Duration, Instant};

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
pub(crate) struct Bft<T: BftSupport> {
    // channel
    msg_receiver: Receiver<BftMsg>,
    msg_sender: Sender<BftMsg>,
    timer_seter: Sender<TimeoutInfo>,
    timer_notity: Receiver<TimeoutInfo>,
    // bft-core params
    height: u64,
    round: u64,
    step: Step,
    block_hash: Option<Hash>,
    lock_status: Option<LockStatus>,
    height_filter: HashMap<Address, Instant>,
    round_filter: HashMap<Address, Instant>,
    last_commit_round: Option<u64>,
    last_commit_block_hash: Option<Hash>,
    authority_manage: AuthorityManage,
    params: BftParams,
    htime: Instant,
    // caches
    feeds: HashMap<u64, Vec<u8>>,
    verify_results: HashMap<Hash, bool>,
    proof: Option<Proof>,
    proposals: ProposalCollector,
    votes: VoteCollector,
    wal_log: Wal,
    // user define
    function: T,
    consensus_power: bool,
}

impl<T> Bft<T>
where
    T: BftSupport + Send + 'static,
{
    /// A function to start a BFT state machine.
    pub(crate) fn start(r: Receiver<BftMsg>, s: Sender<BftMsg>, f: T, local_address: Address, wal_path: &str) {
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
        let mut engine = Bft::initialize(r, s, bft2timer, bft4timer, f, local_address, wal_path);

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
                        //TODO: 将 timeout_process 也改成返回 BftResult
                        engine.timeout_process(&ok_timer);
                    }

                    if let Ok(ok_msg) = get_msg {
                        if let Err(error) = engine.process(ok_msg){
                            error!("Bft process encounters {:?}!", error);
                        }
                    }
                }
            })
            .unwrap();
    }

    fn initialize(
        r: Receiver<BftMsg>,
        s: Sender<BftMsg>,
        ts: Sender<TimeoutInfo>,
        tn: Receiver<TimeoutInfo>,
        f: T,
        local_address: Hash,
        wal_path: &str,
    ) -> Self {
        info!("BFT State Machine Launched.");
        Bft {
            msg_receiver: r,
            msg_sender: s,
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
            proof: None,
            authority_manage: AuthorityManage::new(),
            proposals: ProposalCollector::new(),
            votes: VoteCollector::new(),
            wal_log: Wal::new(wal_path).unwrap(),
            function: f,
            consensus_power: false,
        }
    }

    fn reset(&mut self) {
        self.height = INIT_HEIGHT;
        self.round = INIT_ROUND;
        self.step = Step::default();
        self.block_hash = None;
        self.lock_status = None;
        self.height_filter.clear();
        self.round_filter.clear();
        self.last_commit_round = None;
        self.last_commit_block_hash = None;
        self.htime = Instant::now();
        self.feeds.clear();
        self.verify_results.clear();
        self.proof = None;
        self.authority_manage = AuthorityManage::new();
        self.proposals = ProposalCollector::new();
        self.votes = VoteCollector::new();
        //TODO: 将之前的 wal 文件备份
        let wal_path = &self.wal_log.dir;
        let _ = fs::remove_dir_all(wal_path);
        self.wal_log = Wal::new(wal_path).unwrap();
        self.consensus_power = false;
    }

    fn load_wal_log(&mut self) {
        // TODO: should prevent saving wal
        info!("Cita-bft starts to load wal log!");
        let vec_buf = self.wal_log.load();
        for (log_type, msg) in vec_buf {
            match log_type {
                LogType::Proposal => {
                    info!("Cita-bft loads proposal!");
                    self.msg_sender.send(BftMsg::Proposal(msg)).unwrap();
                }
                LogType::Vote => {
                    info!("Cita-bft loads vote message!");
                    self.msg_sender.send(BftMsg::Vote(msg)).unwrap();
                }
                LogType::Feed => {
                    info!("Cita-bft loads feed message!");
                    self.msg_sender.send(BftMsg::Feed(rlp::decode(&msg).unwrap())).unwrap();
                }
                LogType::Status => {
                    info!("Cita-bft loads status message!");
                    self.msg_sender.send(BftMsg::Status(rlp::decode(&msg).unwrap())).unwrap();
                }
                #[cfg(feature = "verify_req")]
                LogType::VerifyResp => {
                    info!("Cita-bft loads verify_resp message!");
                    self.msg_sender.send(BftMsg::VerifyResp(rlp::decode(&msg).unwrap())).unwrap();
                }
            }
        }
        info!("Cita-bft successfully processes the whole wal log!");
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
    fn cal_above_threshold(&self, count: u64) -> bool {
        let weight: Vec<u64> = self.authority_manage.authorities.iter().map(|node| node.vote_weight as u64).collect();
        let weight_sum: u64 = weight.iter().sum();
        count * 3 > weight_sum * 2
    }

    #[inline]
    fn cal_all_vote(&self, count: u64) -> bool {
        let weight: Vec<u64> = self.authority_manage.authorities.iter().map(|node| node.vote_weight as u64).collect();
        let weight_sum: u64 = weight.iter().sum();
        count == weight_sum
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
        let _ = self.wal_log.set_height(new_height);

        self.height = new_height;
        self.round = 0;
        self.htime = Instant::now();
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

    fn retransmit_vote(&self, round: u64) {
        info!(
            "Some nodes are at low height, retransmit votes of height {:?}, round {:?}",
            self.height - 1,
            round
        );

        debug!(
            "Retransmit votes to proposal {:?}",
            self.last_commit_block_hash.clone().unwrap()
        );

        let prevote = Vote{
            vote_type: VoteType::Prevote,
            height: self.height - 1,
            round,
            block_hash: self.last_commit_block_hash.clone().unwrap(),
            voter: self.params.address.clone(),
        };

        let signed_prevote = self.build_signed_vote(&prevote);
        self.send_bft_msg(BftMsg::Vote(rlp::encode(&signed_prevote)));

        let precommit = Vote{
            vote_type: VoteType::Precommit,
            height: self.height - 1,
            round,
            block_hash: self.last_commit_block_hash.clone().unwrap(),
            voter: self.params.address.clone(),
        };
        let signed_precommit = self.build_signed_vote(&precommit);
        self.send_bft_msg(BftMsg::Vote(rlp::encode(&signed_precommit)));
    }

    fn determine_height_filter(&self, voter: &Address) -> (bool, bool) {
        let mut add_flag = false;
        let mut trans_flag = false;

        if let Some(ins) = self.height_filter.get(voter) {
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

    fn determine_round_filter(&self, voter: &Address) -> (bool, bool) {
        let mut add_flag = false;
        let mut trans_flag = false;

        if let Some(ins) = self.round_filter.get(voter) {
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
        let authorities = &self.authority_manage.authorities;
        if authorities.is_empty() {
            error!("The Authority List is Empty!");
            return false;
        }

        let nonce = self.height + self.round;
        let weight: Vec<u64> = authorities.iter().map(|node| node.proposal_weight as u64).collect();
        let proposer: &Address = &authorities.get(get_proposer(nonce, &weight)).unwrap().address;
        if self.params.address == *proposer {
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
            && (self.feeds.get(&self.height).is_none()
            || self.proof.is_none() || (self.proof.iter().next().unwrap().height != self.height - 1))
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
                self.lock_status.clone().unwrap().block_hash
            );
            let lock_status = self.lock_status.clone().unwrap();
            let lock_round = lock_status.round;
            let lock_votes = lock_status.votes;
            //TODO: bug: the lock_proposal may be flushed out
            let lock_proposal = self.proposals.get_proposal(self.height, lock_round).unwrap();

            let block = lock_proposal.block;
            let block_hash = self.function.crypt_hash(&block);
            let signature = self.function.sign(&block_hash).unwrap();

            let proposal = Proposal{
                height: self.height,
                round: self.round,
                block,
                proof: lock_proposal.proof,
                lock_round: Some(lock_round),
                lock_votes,
                proposer: self.params.address.clone(),
            };

            let signed_proposal = SignedProposal{
                proposal,
                signature,
            };
            BftMsg::Proposal(rlp::encode(&signed_proposal))

        } else {
            // if is not locked, transmit the cached proposal
            let block = self.feeds.get(&self.height).unwrap().clone();
            let block_hash = self.function.crypt_hash(&block);
            self.block_hash = Some(block_hash.clone());
            trace!(
                "Proposal at height {:?}, round {:?}, is {:?}",
                self.height,
                self.round,
                &block_hash,
            );

            let proposal = Proposal{
                height: self.height,
                round: self.round,
                block,
                proof: self.proof.clone().unwrap(),
                lock_round: None,
                lock_votes: Vec::new(),
                proposer: self.params.address.clone(),
            };
            let proposal_encode = rlp::encode(&proposal);
            let proposal_hash = self.function.crypt_hash(&proposal_encode);
            let signature = self.function.sign(&proposal_hash).unwrap();

            let signed_proposal = SignedProposal{
                proposal,
                signature,
            };

            BftMsg::Proposal(rlp::encode(&signed_proposal))
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
                proposal.height, proposal.round, self.height, self.round, self.function.crypt_hash(&proposal.block)
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
        let block_hash = self.function.crypt_hash(&proposal.block);

        if proposal.lock_round.is_some()
            && (self.lock_status.is_none()
                || self.lock_status.clone().unwrap().round <= proposal.lock_round.unwrap())
        {
            // receive a proposal with a later PoLC
            debug!(
                "Receive a proposal with the PoLC that proposal is {:?}, lock round is {:?}, lock votes are {:?}",
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
            debug!(
                "Receive a proposal without PoLC, the proposal is {:?}",
                block_hash
            );
            self.block_hash = Some(block_hash);
        } else {
            debug!("Receive a proposal that the PoLC is earlier than mine");
            return;
        }
    }

    fn transmit_prevote(&mut self) {
        let block_hash = if let Some(lock_status) = self.lock_status.clone() {
            lock_status.block_hash
        } else if let Some(block_hash) = self.block_hash.clone() {
            block_hash
        } else {
            Vec::new()
        };

        trace!(
            "Transmit prevote at height {:?}, round {:?}",
            self.height,
            self.round
        );

        let vote = Vote{
            vote_type: VoteType::Prevote,
            height: self.height,
            round: self.round,
            block_hash: block_hash.clone(),
            voter: self.params.address.clone(),
        };
        let signed_vote = self.build_signed_vote(&vote);

        let vote_weight = self.get_vote_weight(self.height, &vote.voter);
        let _ = self.votes.add(&signed_vote, vote_weight, self.height);

        let msg = BftMsg::Vote(rlp::encode(&signed_vote));
        self.send_bft_msg(msg);
        debug!("Prevote to {:?}", block_hash);

        self.set_timer(
            self.params.timer.get_prevote() * TIMEOUT_RETRANSE_COEF,
            Step::Prevote,
        );
    }

    fn build_signed_vote(&self, vote: &Vote) -> SignedVote{
        let encode =  rlp::encode(vote);
        let hash = self.function.crypt_hash(&encode);
        let signature = self.function.sign(&hash).unwrap();
        SignedVote {
            vote: vote.clone(),
            signature,
        }
    }

    fn get_vote_weight(&self, height: u64, address: &Address) -> u64 {
        if height != self.height{
            return 1u64;
        }
        if let Some(node) = self.authority_manage.authorities.iter()
            .find(|node| node.address == *address){
            return node.vote_weight as u64;
        }
        1u64
    }

    fn handle_vote(&mut self, vote: Vote) -> bool {
        info!(
            "Receive a {:?} vote of height {:?}, round {:?}, to {:?}, from {:?}",
            vote.vote_type, vote.height, vote.round, vote.block_hash, vote.voter
        );

        if vote.height == self.height - 1 {
            if self.last_commit_round.is_some() && vote.round >= self.last_commit_round.unwrap() {
                // deal with height fall behind one, round ge last commit round
                let voter = vote.voter.clone();
                let (add_flag, trans_flag) = self.determine_height_filter(&voter);

                if add_flag {
                    self.height_filter
                        .entry(voter)
                        .or_insert_with(Instant::now);
                }
                if trans_flag {
                    self.retransmit_vote(vote.round);
                }
            }
            return false;
        } else if vote.height == self.height && self.round != 0 && vote.round == self.round - 1 {
            // deal with equal height, round fall behind
            let voter = vote.voter.clone();
            let (add_flag, trans_flag) = self.determine_round_filter(&voter);

            if add_flag {
                self.round_filter.entry(voter).or_insert_with(Instant::now);
            }
            if trans_flag {
                let precommit = Vote{
                    vote_type: VoteType::Precommit,
                    height: vote.height,
                    round: vote.round,
                    block_hash: Vec::new(),
                    voter: self.params.address.clone(),
                };
                let signed_precommit = self.build_signed_vote(&precommit);

                info!("Some nodes fall behind, send nil vote to help them pursue");
                self.send_bft_msg(BftMsg::Vote(rlp::encode(&signed_precommit)));
            }
            return false;
        } else if vote.height == self.height
            && vote.round >= self.round
        {
            return true;
        }
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
                            trace!(
                                "Receive over 2/3 prevote to nil at height {:?}, round {:?}",
                                self.height,
                                self.round
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

    fn transmit_precommit(&mut self) {
        let block_hash = if let Some(lock_status) = self.lock_status.clone() {
            lock_status.block_hash
        } else {
            self.block_hash = None;
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
            block_hash: block_hash.clone(),
            voter: self.params.address.clone(),
        };
        let signed_vote = self.build_signed_vote(&vote);

        let vote_weight = self.get_vote_weight(self.height, &vote.voter);
        let _ = self.votes.add(&signed_vote, vote_weight, self.height);

        let msg = BftMsg::Vote(rlp::encode(&signed_vote));
        self.send_bft_msg(msg);
        debug!("Precommit to {:?}", block_hash);

        self.set_timer(
            self.params.timer.get_precommit() * TIMEOUT_RETRANSE_COEF,
            Step::Precommit,
        );
    }

    fn check_precommit_count(&mut self) -> i8 {
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
                        self.set_polc(&hash, &precommit_set);
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
//    pub struct Commit {
/////
//pub height: u64,
/////
//pub block: Vec<u8>,
/////
//pub pre_hash: Hash,
/////
//pub proof: Proof,
/////
//pub address: Address,
//}

    fn proc_commit(&mut self) {
        let lock_status = self.lock_status.clone().expect("No lock when commit!");

        let proof = self.generate_proof(lock_status.clone());
        self.proof = Some(proof);

        let proposal = self.proposals.get_proposal(self.height, self.round).unwrap();

        let commit = Commit{
            height: self.height,
            block: proposal.block.clone(),
            proof: proposal.proof,
            address: self.params.address.clone(),
        };

        self.function.commit(commit);

        info!(
            "Commit {:?} at height {:?}, consensus time {:?}",
            lock_status.block_hash,
            self.height,
            Instant::now() - self.htime
        );

        self.last_commit_round = Some(self.round);
        self.last_commit_block_hash = Some(self.function.crypt_hash(&proposal.block));
    }

    fn generate_proof(&mut self, lock_status: LockStatus) -> Proof {
        let block_hash = lock_status.block_hash;
        let lock_votes = lock_status.votes;
        let precommit_votes: HashMap<Address, Signature> = lock_votes.into_iter().map(|signed_vote| (signed_vote.vote.voter, signed_vote.signature)).collect();
        Proof{
            height: self.height,
            round: lock_status.round,
            block_hash,
            precommit_votes,
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
            "Get PoLC at height {:?}, round {:?}, on proposal {:?}",
            self.height,
            self.round,
            hash.to_owned()
        );
    }

    fn clean_polc(&mut self) {
        self.block_hash = None;
        self.lock_status = None;
        trace!(
            "Clean PoLC at height {:?}, round {:?}",
            self.height,
            self.round
        );
    }

    fn try_handle_status(&mut self, status: Status) -> bool {
        // receive a rich status that height ge self.height is the only way to go to new height
        if status.height >= self.height {
            if status.height > self.height {
                // recvive higher status, clean last commit info then go to new height
                self.last_commit_block_hash = None;
                self.last_commit_round = None;
            }
            // goto new height directly and update authorty list

            self.authority_manage.receive_authorities_list(status.height, &status.authority_list);

            if self.consensus_power &&
                !status.authority_list.iter().any(|node| node.address == self.params.address) {
                info!("Cita-bft loses consensus power in height {} and stops the bft-rs process!", status.height);
                self.consensus_power = false;
            } else if !self.consensus_power
                && status.authority_list.iter().any(|node| node.address == self.params.address) {
                info!("Cita-bft accesses consensus power in height {} and starts the bft-rs process!", status.height);
                self.consensus_power = true;
            }

            if let Some(interval) = status.interval {
                // update the bft interval
                self.params.timer.set_total_duration(interval);
            }

            self.goto_new_height(status.height + 1);

            self.flush_cache();

            info!(
                "Receive rich status, goto new height {:?}",
                status.height + 1
            );
            return true;
        }
        false
    }

    fn flush_cache(&mut self) {
        let proposals = &mut self.proposals.proposals;
        let round_proposals = proposals.get_mut(&self.height);
        let votes = &mut self.votes.votes;
        let round_votes = votes.get_mut(&self.height);
        let mut round_collector = RoundCollector::new();
        if !round_votes.is_none() {
            round_collector = round_votes.unwrap().clone();
            self.votes.remove(self.height);
        }

        if let Some(round_proposals) = round_proposals {
            for (_, signed_proposal) in round_proposals.round_proposals.iter() {
                self.msg_sender.send(BftMsg::Proposal(rlp::encode(signed_proposal))).unwrap();
            }
        };

        for (_, step_votes) in round_collector.round_votes.iter() {
            for (_, vote_set) in step_votes.step_votes.iter() {
                for (_, signed_vote) in vote_set.votes_by_sender.iter() {
                    self.msg_sender.send(BftMsg::Vote(rlp::encode(signed_vote))).unwrap();
                }
            }
        }
    }

    fn save_verify_res(&mut self, block_hash: &[u8], is_pass: bool) {
        if self.verify_results.contains_key(block_hash) {
            if is_pass != *self.verify_results.get(block_hash).unwrap() {
                error!("The verify results of {:?} are conflict!", block_hash);
                return;
            }
        }
        self.verify_results.entry(block_hash.to_vec()).or_insert(is_pass);
    }

    fn new_round_start(&mut self) {
        if self.step != Step::ProposeWait {
            info!("Start height {:?}, round{:?}", self.height, self.round);
        }
        if self.is_proposer() {
            self.function.get_block(self.height);
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

    fn check_and_save_proposal(&mut self, signed_proposal: &SignedProposal, need_wal: bool) -> BftResult<()> {
        let proposal = &signed_proposal.proposal;
        let height = proposal.height;
        let round = proposal.round;

        if height < self.height - 1 {
            warn!("The height of proposal is {} which is obsolete compared to self.height {}!", height, self.height);
            return Err(BftError::ObsoleteMsg);
        }

        let block = &proposal.block;
        let block_hash = self.function.crypt_hash(block);

        let proposal_encode = rlp::encode(proposal);
        let proposal_hash = self.function.crypt_hash(&proposal_encode);
        let address = self.function.check_signature(&signed_proposal.signature, &proposal_hash).ok_or(BftError::CheckSigFailed)?;
        if proposal.proposer != address {
            return Err(BftError::MismatchingProposer);
        }

        // check_prehash should involved in check_block
        self.check_block(block, height)?;

        if need_wal {
            let encode: Vec<u8> = rlp::encode(signed_proposal);
            self.wal_log.save(height, LogType::Proposal, &encode).or(Err(BftError::SaveWalErr))?
        }

        self.proposals.add(&proposal);

        if height > self.height {
            return Err(BftError::HigherMsg);
        }

        self.check_proposer(height, round, &address)?;
        self.check_lock_votes(proposal, &block_hash)?;

        if height < self.height {
            return Ok(());
        }

        self.check_proof(height, &proposal.proof)?;

        Ok(())
    }

    fn check_and_save_status(&mut self, status: &Status, need_wal: bool) -> BftResult<()> {
        let height = status.height;
        if height < self.height {
            warn!("The height of rich_status is {} which is obsolete compared to self.height {}!", height, self.height);
            return Err(BftError::ObsoleteMsg);
        }
        if need_wal {
            let msg: Vec<u8> = rlp::encode(status);
            self.wal_log.save(height + 1, LogType::Status, &msg).or( Err(BftError::SaveWalErr))?;
        }

        Ok(())
    }

    #[cfg(feature = "verify_req")]
    fn check_and_save_verify_resp(&mut self, verify_resp: &VerifyResp, need_wal: bool) -> BftResult<()> {
        if need_wal {
            let msg: Vec<u8> = rlp::encode(verify_resp);
            self.wal_log.save(height, LogType::VerifyResp, &msg).or( Err(BftError::SaveWalErr))?;
        }
        self.save_verify_res(&verify_resp.block_hash, verify_resp.is_pass);

        Ok(())
    }

    fn check_and_save_feed(&mut self, feed: Feed, need_wal: bool) -> BftResult<()> {
        let height = feed.height;
        if self.height != 0 && height < self.height - 1 {
            warn!("the height of block_txs is {}, while self.height is {}", height, self.height);
            return Err(BftError::ObsoleteMsg);
        }
        if need_wal {
            let msg: Vec<u8> = rlp::encode(&feed);
            self.wal_log.save(height + 1, LogType::Feed, &msg).or(Err(BftError::SaveWalErr))?;
        }

        let block_hash = self.function.crypt_hash(&feed.block);
        self.save_verify_res(&block_hash, true);

        self.feeds.insert(height, feed.block);

        if height > self.height - 1{
            return Err(BftError::HigherMsg);
        }
        Ok(())
    }


    fn check_and_save_vote(&mut self, signed_vote: &SignedVote, need_wal: bool) -> BftResult<()> {
        let vote = &signed_vote.vote;
        let height = vote.height;
        if height < self.height - 1 {
            warn!("The height of raw_bytes is {} which is obsolete compared to self.height {}!", height, self.height);
            return Err(BftError::ObsoleteMsg);
        }

        let vote_encode = rlp::encode(vote);
        let vote_hash = self.function.crypt_hash(&vote_encode);
        let address = self.function.check_signature(&signed_vote.signature, &vote_hash).ok_or(BftError::CheckSigFailed)?;
        if vote.voter != address {
            return Err(BftError::MismatchingVoter);
        }

        if need_wal {
            let encode: Vec<u8> = rlp::encode(signed_vote);
            self.wal_log.save(height, LogType::Vote, &encode).or(Err(BftError::SaveWalErr))?;
        }

        let vote_weight = self.get_vote_weight(vote.height, &vote.voter);
        self.votes.add(&signed_vote, vote_weight, self.height);

        if height > self.height {
            return Err(BftError::HigherMsg);
        }

        self.check_voter(height, &vote.voter)?;

        Ok(())
    }

    fn check_block(&mut self, block: &[u8], height: u64) -> BftResult<()> {
        let block_hash = self.function.crypt_hash(block);
        // search cache first
        if let Some(verify_res) = self.verify_results.get(&block_hash) {
            if *verify_res {
                return Ok(());
            }else {
                return Err(BftError::CheckBlockFailed);
            }
        }

        #[cfg(not(feature = "verify_req"))]
            {
                if self.function.check_block(block, height) {
                    self.save_verify_res(&block_hash, true);
                    Ok(())
                } else {
                    self.save_verify_res(&block_hash, false);
                    Err(BftError::CheckBlockFailed)
                }
            }


        #[cfg(feature = "verify_req")]
            {
                match self.function.check_block(block, height) {
                    CheckResp::NoPass => {
                        self.save_verify_res(&block_hash, false);
                        Err(BftError::CheckBlockFailed)
                    },
                    CheckResp::PartialPass => {
                        Ok(())
                    },
                    CheckResp::CompletePass => {
                        self.save_verify_res(&block_hash, true);
                        Ok(())
                    }
                }
            }


    }

    fn check_proof(&mut self, height: u64, proof: &Proof) -> BftResult<()> {
        if height != self.height {
            error!("The height {} is less than self.height {}, which should not happen!", height, self.height);
            return Err(BftError::ShouldNotHappen);
        }

        let p = &self.authority_manage;
        let mut authorities = &p.authorities;
        if height == p.authority_h_old {
            authorities = &p.authorities_old;
        }

        if !self.check_proof_only(&proof, height, authorities){
            return Err(BftError::CheckProofFailed);
        }

        if self.proof.is_none() || self.proof.iter().next().unwrap().height != height - 1 {
            info!("Cita-bft sets self.proof from received signed_proposal!");
            self.proof = Some(proof.clone());
        }

        Ok(())
    }

    fn check_proof_only(&self, proof: &Proof, height: u64, authorities: &[Node]) -> bool {
        if height == 0 {
            return true;
        }
        if height != proof.height {
            return false;
        }

        let weight: Vec<u64> = authorities.iter().map(|node| node.vote_weight as u64).collect();
        let vote_addresses : Vec<Address> = proof.precommit_votes.iter().map(|(voter, _)| voter.clone()).collect();
        let votes_weight: Vec<u64> = authorities.iter()
            .filter(|node| vote_addresses.contains(&node.address))
            .map(|node| node.vote_weight as u64).collect();
        let weight_sum: u64 = weight.iter().sum();
        let vote_sum: u64 = votes_weight.iter().sum();
        if vote_sum * 3 > weight_sum * 2 {
            return false;
        }

        proof.precommit_votes.iter().all(|(voter, sig)| {
            if authorities.iter().any(|node| node.address == *voter) {
                let vote = Vote{
                    vote_type: VoteType::Precommit,
                    height,
                    round: proof.round,
                    block_hash: proof.block_hash.clone(),
                    voter: voter.clone(),
                };
                let msg = rlp::encode(&vote);
                if let Some(address) = self.function.check_signature(sig, &self.function.crypt_hash(&msg)) {
                    return address == *voter;
                }
            }
            false
        })
    }

    fn check_lock_votes(&mut self, proposal: &Proposal, block_hash: &[u8]) -> BftResult<()> {
        let height = proposal.height;
        if height < self.height - 1 || height > self.height{
            error!("The proposal's height is {} compared to self.height {}, which should not happen!", height, self.height);
            return Err(BftError::ShouldNotHappen);
        }

        let mut map = HashMap::new();
        if let Some(lock_round) = proposal.lock_round {
            for signed_vote in &proposal.lock_votes {
                let voter = self.check_vote(height, lock_round, block_hash, signed_vote)?;
                if let Some(_) = map.insert(voter, 1) {
                    return Err(BftError::RepeatLockVote);
                }
            }
        } else {
            return Ok(());
        }

        let p = &self.authority_manage;
        let mut authorities = &p.authorities;
        if height == p.authority_h_old {
            authorities = &p.authorities_old;
        }

        let weight: Vec<u64> = authorities.iter().map(|node| node.vote_weight as u64).collect();
        let vote_addresses : Vec<Address> = proposal.lock_votes.iter().map(|signed_vote| signed_vote.vote.voter.clone()).collect();
        let votes_weight: Vec<u64> = authorities.iter()
            .filter(|node| vote_addresses.contains(&node.address))
            .map(|node| node.vote_weight as u64).collect();
        let weight_sum: u64 = weight.iter().sum();
        let vote_sum: u64 = votes_weight.iter().sum();
        if vote_sum * 3 > weight_sum * 2 {
            return Ok(());
        }
        Err(BftError::NotEnoughVotes)
    }

    fn check_vote(&mut self, height: u64, round: u64, block_hash: &[u8], signed_vote: &SignedVote) -> BftResult<Address> {
        if height < self.height - 1 {
            error!("The vote's height {} is less than self.height {} - 1, which should not happen!", height, self.height);
            return Err(BftError::ShouldNotHappen);
        }

        let vote = &signed_vote.vote;
        if vote.height != height || vote.round != round {
            return Err(BftError::VoteError);
        }

        if vote.block_hash != block_hash.to_vec() {
            error!("The lock votes of proposal {:?} contains vote for other proposal hash {:?}!", block_hash, vote.block_hash);
            return Err(BftError::MismatchingVote);
        }

        let p = &self.authority_manage;
        let mut authorities = &p.authorities;
        if height == p.authority_h_old {
            info!("Cita-bft sets the authority manage with old authorities!");
            authorities = &p.authorities_old;
        }

        let voter = &vote.voter;
        if !authorities.iter().any(|node| &node.address == voter) {
            error!("The lock votes contains vote with invalid voter {:?}!", voter);
            return Err(BftError::InvalidVoter);
        }

        let signature = &signed_vote.signature;
        let vote_encode = rlp::encode(vote);
        let vote_hash = self.function.crypt_hash(&vote_encode);
        let address = self.function.check_signature(signature, &vote_hash).ok_or(BftError::CheckSigFailed)?;
        if &address != voter {
            error!("The address recovers from the signature is {:?} which is mismatching with the sender {:?}!", &address, &voter);
            return Err(BftError::MismatchingVoter);
        }

        let vote_weight = self.get_vote_weight(height, &voter);
        self.votes.add(&signed_vote, vote_weight, self.height);
        Ok(address)
    }

    fn check_proposer(&self, height: u64, round: u64, address: &Address) -> BftResult<()> {
        if height < self.height - 1 || height > self.height{
            error!("The proposer's height is {} compared to self.height {}, which should not happen!", height, self.height);
            return Err(BftError::ShouldNotHappen);
        }
        let p = &self.authority_manage;
        let mut authorities = &p.authorities;
        if height == p.authority_h_old {
            info!("Cita-bft sets the authority manage with old authorities!");
            authorities = &p.authorities_old;
        }
        if authorities.len() == 0 {
            error!("The authority manage is empty!");
            return Err(BftError::EmptyAuthManage);
        }
        let nonce = height + round;
        let weight: Vec<u64> = authorities.iter().map(|node| node.proposal_weight as u64).collect();
        let proposer: &Address = &authorities.get(get_proposer(nonce, &weight)).unwrap().address;
        if proposer == address {
            Ok(())
        } else {
            error!("The proposer {:?} is invalid, while the rightful proposer is {:?}", address, proposer);
            Err(BftError::InvalidProposer)
        }
    }

    fn check_voter(&self, height: u64, address: &Address) -> BftResult<()> {
        if height < self.height - 1 || height > self.height{
            error!("The height is {} compared to self.height {}, which should not happen!", height, self.height);
            return Err(BftError::ShouldNotHappen);
        }
        let p = &self.authority_manage;
        let mut authorities = &p.authorities;
        if height == p.authority_h_old {
            info!("Cita-bft sets the authority manage with old authorities!");
            authorities = &p.authorities_old;
        }

        if !authorities.iter().any(|node| node.address == *address) {
            error!("The lock votes contains vote with invalid voter {:?}!", address);
            return Err(BftError::InvalidVoter);
        }

        Ok(())
    }

    fn process(&mut self, msg: BftMsg) -> BftResult<()>{
        match msg {
            BftMsg::Proposal(encode) => {
                if self.consensus_power {
                    let signed_proposal: SignedProposal = rlp::decode(&encode).or(Err(BftError::DecodeErr))?;
                    self.check_and_save_proposal(&signed_proposal, true)?;

                    let proposal = signed_proposal.proposal;
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
            }

            BftMsg::Vote(encode) => {
                if self.consensus_power {
                    let signed_vote: SignedVote = rlp::decode(&encode).or(Err(BftError::DecodeErr))?;
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
                                let precommit_result = self.check_precommit_count();

                                if precommit_result == PRECOMMIT_ON_NOTHING {
                                    // only receive +2/3 precommits might lead BFT to PrecommitWait
                                    self.change_to_step(Step::PrecommitWait);
                                }

                                if precommit_result == PRECOMMIT_ON_NIL {
                                    // receive +2/3 on nil, goto next round directly
                                    if self.lock_status.is_none() {
                                        self.block_hash = None;
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
                        return Err(BftError::MsgTypeErr);
                    }
                }
            }

            BftMsg::Feed(feed) => {
                self.check_and_save_feed(feed, true)?;

                if self.step == Step::ProposeWait {
                    self.new_round_start();
                }
            }

            BftMsg::Status(status) => {
                self.check_and_save_status(&status, true)?;

                if self.try_handle_status(status) {
                    self.new_round_start();
                }
            }

            #[cfg(feature = "verify_req")]
            BftMsg::VerifyResp(verify_resp) => {
                self.check_and_save_verify_resp(&verify_resp, true)?;

                if self.step == Step::VerifyWait {
                    // next do precommit
                    self.change_to_step(Step::Precommit);
                    if self.check_verify() == VerifyResult::Undetermined {
                        self.change_to_step(Step::VerifyWait);
                        break;
                    }
                    self.transmit_precommit();
                    let precommit_result = self.check_precommit_count();

                    if precommit_result == PRECOMMIT_ON_NOTHING {
                        // only receive +2/3 precommits might lead BFT to PrecommitWait
                        self.change_to_step(Step::PrecommitWait);
                    }

                    if precommit_result == PRECOMMIT_ON_NIL {
                        if self.lock_status.is_none() {
                            self.block_hash = None;
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

            BftMsg::Pause => self.consensus_power = false,

            BftMsg::Start => self.consensus_power = true,

            BftMsg::Snapshot(snap_shot) => {
                self.reset();
                self.height = snap_shot.proof.height;
                self.proof = Some(snap_shot.proof);
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
                let precommit_result = self.check_precommit_count();

                if precommit_result == PRECOMMIT_ON_NOTHING {
                    // only receive +2/3 precommits might lead BFT to PrecommitWait
                    self.change_to_step(Step::PrecommitWait);
                }

                if precommit_result == PRECOMMIT_ON_NIL {
                    if self.lock_status.is_none() {
                        self.block_hash = None;
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
                        self.block_hash = None;
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