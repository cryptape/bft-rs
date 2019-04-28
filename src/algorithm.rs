use crate::*;
use crate::{
    collectors::{ProposalCollector, VoteCollector, VoteSet},
    error::{handle_error, BftError, BftResult},
    objects::*,
    params::BftParams,
    timer::{TimeoutInfo, WaitTimer},
    wal::Wal,
};

use crossbeam::crossbeam_channel::{unbounded, Receiver, RecvError, Sender};
//use crossbeam_utils::thread as cross_thread;
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
    pub(crate) height: Height,
    pub(crate) round: Round,
    pub(crate) step: Step,
    pub(crate) block_hash: Option<Hash>,
    pub(crate) lock_status: Option<LockStatus>,
    pub(crate) height_filter: HashMap<Address, Instant>,
    pub(crate) round_filter: HashMap<Address, Instant>,
    pub(crate) last_commit_round: Option<Round>,
    pub(crate) last_commit_block_hash: Option<Hash>,
    pub(crate) authority_manage: AuthorityManage,
    pub(crate) params: BftParams,
    pub(crate) htime: Instant,
    // caches
    pub(crate) feed: Option<Block>,
    pub(crate) status: Option<Status>,
    pub(crate) verify_results: HashMap<Round, bool>,
    pub(crate) proof: Proof,
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

        let mut engine = Bft::initialize(s, r, bft2timer, bft4timer, f, local_address, wal_path);

        // start timer module.
        let _timer_thread = thread::Builder::new()
            .name("bft_timer".to_string())
            .spawn(move || {
                let timer = WaitTimer::new(timer2bft, timer4bft);
                timer.start();
            })
            .expect("Bft starts time-thread failed!");

        // start main loop module.
        let _main_thread = thread::Builder::new()
            .name("main_loop".to_string())
            .spawn(move || {
                engine.load_wal_log();

                loop {
                    let mut get_timer_msg = Err(RecvError);
                    let mut get_msg = Err(RecvError);

                    select! {
                        recv(engine.timer_notity) -> msg => get_timer_msg = msg,
                        recv(engine.msg_receiver) -> msg => get_msg = msg,
                    }

                    let mut result = Ok(());
                    if let Ok(msg) = get_timer_msg {
                        result = engine.timeout_process(msg, true);
                    }
                    if let Ok(msg) = get_msg {
                        result = engine.process(msg, true);
                    }
                    handle_error(result);
                }
            })
            .expect("Bft starts main-thread failed!");
    }

    pub(crate) fn process(&mut self, msg: BftMsg, need_wal: bool) -> BftResult<()> {
        match msg {
            BftMsg::Proposal(encode) => {
                if self.consensus_power {
                    let signed_proposal: SignedProposal = rlp::decode(&encode).map_err(|e| {
                        BftError::DecodeErr(format!("signed_proposal encounters {:?}", e))
                    })?;
                    trace!("Bft receives {:?}", &encode);
                    self.check_and_save_proposal(&signed_proposal, &encode, need_wal)?;

                    let proposal = signed_proposal.proposal;
                    if self.step <= Step::ProposeWait {
                        self.handle_proposal(&proposal)?;
                        self.set_proposal(proposal);
                        if self.step == Step::ProposeWait {
                            self.transmit_prevote()?;
                        }
                    }
                }
            }

            BftMsg::Vote(encode) => {
                if self.consensus_power {
                    let signed_vote: SignedVote = rlp::decode(&encode).map_err(|e| {
                        BftError::DecodeErr(format!("signed_vote encounters {:?}", e))
                    })?;
                    trace!("Bft receives {:?}", &encode);
                    self.check_and_save_vote(&signed_vote, need_wal)?;

                    let vote = signed_vote.vote;
                    match vote.vote_type {
                        VoteType::Prevote => {
                            if self.step <= Step::PrevoteWait {
                                self.handle_vote(vote)?;
                                if self.step >= Step::Prevote && self.check_prevote_count() {
                                    self.change_to_step(Step::PrevoteWait);
                                }
                            }
                        }
                        VoteType::Precommit => {
                            if self.step < Step::Precommit {
                                self.handle_vote(vote.clone())?;
                            }
                            if self.step == Step::Precommit || self.step == Step::PrecommitWait {
                                self.handle_vote(vote)?;
                                self.handle_precommit()?;
                            }
                        }
                    }
                }
            }

            BftMsg::Feed(feed) => {
                trace!("Bft receives feed {:?}", &feed);
                self.check_and_save_feed(&feed, need_wal)?;

                if self.step == Step::ProposeWait {
                    self.new_round_start(false)?;
                }
            }

            BftMsg::Status(status) => {
                info!("Bft receives status {:?}", &status);
                self.check_and_save_status(&status, need_wal)?;
                self.handle_status(status)?;
            }

            #[cfg(feature = "verify_req")]
            BftMsg::VerifyResp(verify_resp) => {
                trace!("Bft receives verify_resp {:?}", &verify_resp);
                self.check_and_save_verify_resp(&verify_resp, need_wal)?;

                if self.step == Step::VerifyWait {
                    // next do precommit
                    self.change_to_step(Step::Precommit);
                    if self.check_verify() == VerifyResult::Undetermined {
                        self.change_to_step(Step::VerifyWait);
                    } else {
                        self.transmit_precommit()?;
                    }
                }
            }

            BftMsg::Pause => self.consensus_power = false,

            BftMsg::Start => self.consensus_power = true,

            BftMsg::Clear(proof) => {
                trace!("Bft receives clear {:?}", &proof);
                self.clear(proof);
            }
        }

        Ok(())
    }

    pub(crate) fn timeout_process(&mut self, tminfo: TimeoutInfo, need_wal: bool) -> BftResult<()> {
        if tminfo.height < self.height {
            return Err(BftError::ObsoleteTimer(format!(
                "TimeoutInfo height: {} < self.height: {}",
                tminfo.height, self.height
            )));
        }
        if tminfo.height == self.height && tminfo.round < self.round {
            return Err(BftError::ObsoleteTimer(format!(
                "TimeoutInfo round: {} < self.round: {}",
                tminfo.round, self.round
            )));
        }
        if tminfo.height == self.height && tminfo.round == self.round && tminfo.step != self.step {
            return Err(BftError::ObsoleteTimer(format!(
                "TimeoutInfo step: {:?} != self.step: {:?}",
                tminfo.step, self.step
            )));
        }

        if need_wal {
            self.wal_log
                .save(self.height, LogType::TimeOutInfo, &rlp::encode(&tminfo))
                .or(Err(BftError::SaveWalErr(format!("{:?}", &tminfo))))?;
        }

        match tminfo.step {
            Step::ProposeWait => {
                self.change_to_step(Step::Prevote);
                self.transmit_prevote()?;
                if self.check_prevote_count() {
                    self.change_to_step(Step::PrevoteWait);
                }
            }
            Step::Prevote => {
                self.transmit_prevote()?;
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
                        return Ok(());
                    }
                }

                self.transmit_precommit()?;
            }
            Step::Precommit => {
                self.transmit_prevote()?;
                self.transmit_precommit()?;
            }
            Step::PrecommitWait => {
                // receive +2/3 precommits however no proposal reach +2/3
                // then goto next round directly
                self.goto_next_round();
                self.new_round_start(true)?;
            }

            #[cfg(feature = "verify_req")]
            Step::VerifyWait => {
                // clean fsave info
                self.clean_polc();

                // next do precommit
                self.change_to_step(Step::Precommit);
                self.transmit_precommit()?;
            }

            Step::CommitWait => {
                self.set_status(&self.status.clone().unwrap());
                self.goto_new_height(self.height + 1);
                self.new_round_start(true)?;
                self.flush_cache()?;
            }
            _ => error!("Invalid Timeout Info!"),
        }

        Ok(())
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
            feed: None,
            verify_results: HashMap::new(),
            proof: Proof::default(),
            status: None,
            authority_manage: AuthorityManage::new(),
            proposals: ProposalCollector::new(),
            votes: VoteCollector::new(),
            wal_log: Wal::new(wal_path).unwrap(),
            function: f,
            consensus_power: false,
        }
    }

    fn handle_proposal(&self, proposal: &Proposal) -> BftResult<()> {
        if proposal.height == self.height - 1 {
            if self.last_commit_round.is_some() && proposal.round >= self.last_commit_round.unwrap()
            {
                // deal with height fall behind one, round ge last commit round
                self.retransmit_lower_votes(proposal.round)?;
            }
            return Err(BftError::ObsoleteMsg(format!(
                "1 height lower of {:?}",
                proposal
            )));
        } else if proposal.round < self.round {
            return Err(BftError::ObsoleteMsg(format!(
                "lower round of {:?}",
                proposal
            )));
        }

        Ok(())
    }

    fn handle_vote(&mut self, vote: Vote) -> BftResult<()> {
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
                    self.retransmit_lower_votes(vote.round)?;
                }
            }
            return Err(BftError::ObsoleteMsg(format!(
                "1 height lower of {:?}",
                &vote
            )));
        } else if vote.height == self.height && self.round != 0 && vote.round == self.round - 1 {
            // deal with equal height, round fall behind
            let voter = vote.voter.clone();
            let trans_flag = self.filter_round(&voter);

            if trans_flag {
                self.round_filter.insert(voter, Instant::now());
                self.retransmit_nil_precommit(&vote)?;
            }
            return Err(BftError::ObsoleteMsg(format!(
                "1 round lower of {:?}",
                &vote
            )));
        } else if vote.height == self.height && vote.round >= self.round {
            return Ok(());
        }
        Err(BftError::ObsoleteMsg(format!("{:?}", &vote)))
    }

    fn handle_precommit(&mut self) -> BftResult<()> {
        let result = self.check_precommit_count();
        match result {
            PrecommitRes::Above => self.change_to_step(Step::PrecommitWait),
            PrecommitRes::Nil => {
                if self.lock_status.is_none() {
                    self.block_hash = None;
                }
                self.goto_next_round();
                self.new_round_start(true)?;
            }
            PrecommitRes::Proposal => {
                self.change_to_step(Step::Commit);
                self.handle_commit()?;
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_commit(&mut self) -> BftResult<()> {
        let lock_status = self.lock_status.clone().expect("No lock when commit!");

        let proof = self.generate_proof(lock_status.clone());
        self.set_proof(&proof);

        let signed_proposal = self.proposals.get_proposal(self.height, self.round).ok_or(
            BftError::ShouldNotHappen(
                "can not fetch proposal from cache when handle commit".to_string(),
            ),
        )?;
        let proposal = signed_proposal.proposal;

        let commit = Commit {
            height: self.height,
            block: proposal.block.clone(),
            proof,
            address: proposal.proposer.clone(),
        };

        info!(
            "Bft commits {:?} at height {:?}, consumes consensus time {:?}",
            lock_status.block_hash,
            self.height,
            Instant::now() - self.htime
        );

        self.function
            .commit(commit.clone())
            .map_err(|e| BftError::CommitFailed(format!("{:?} of {:?}", e, &commit)))
            .and_then(|status| self.send_bft_msg(BftMsg::Status(status)))?;

        info!("Bft block by commit function");

        self.last_commit_round = Some(self.round);
        self.last_commit_block_hash = Some(self.function.crypt_hash(&proposal.block));
        Ok(())
    }

    fn handle_status(&mut self, status: Status) -> BftResult<()> {
        // commit timeout since pub block to chain,so resending the block
        if self.height > 0 && status.height == self.height - 1 && self.step >= Step::Commit {
            self.handle_commit()?;
        }

        // receive a rich status that height ge self.height is the only way to go to new height
        if status.height >= self.height {
            self.status = Some(status.clone());

            #[cfg(not(feature = "machine_gun"))]
            {
                if status.height == self.height {
                    let cost_time = Instant::now() - self.htime;
                    let interval = self.params.timer.get_total_duration();
                    let mut tv = Duration::new(0, 0);
                    if cost_time < interval {
                        tv = interval - cost_time;
                    }
                    self.change_to_step(Step::CommitWait);
                    self.set_timer(tv, Step::CommitWait);
                    return Ok(());
                }
            }

            if status.height > self.height {
                // recvive higher status, clean last commit info then go to new height
                self.last_commit_block_hash = None;
                self.last_commit_round = None;
            }

            self.set_status(&status);
            self.goto_new_height(status.height + 1);
            self.new_round_start(true)?;
            self.flush_cache()?;

            info!(
                "Bft receives rich status, goto new height {:?}",
                status.height + 1
            );
            return Ok(());
        }
        Err(BftError::ObsoleteMsg(format!("{:?}", &status)))
    }

    fn transmit_proposal(&mut self) -> BftResult<()> {
        if self.lock_status.is_none()
            && (self.feed.is_none() || self.proof.height != self.height - 1)
        {
            // if a proposer find there is no proposal nor lock, goto step proposewait
            let coef = if self.round > PROPOSAL_TIMES_COEF {
                PROPOSAL_TIMES_COEF
            } else {
                self.round
            };

            self.set_timer(
                self.params.timer.get_propose() * 2u32.pow(coef as u32),
                Step::ProposeWait,
            );
            return Err(BftError::NotReady(format!(
                "transmit proposal (feed: {:?}, proof: {:?} lock_status: {:?})",
                self.feed, self.proof, self.lock_status
            )));
        }

        let msg = if self.lock_status.is_some() {
            // if is locked, boradcast the lock proposal
            info!("Bft is ready to transmit locked Proposal");
            let lock_status = self.lock_status.clone().unwrap();
            let lock_round = lock_status.round;
            let lock_votes = lock_status.votes;

            let lock_signed_proposal = self
                .proposals
                .get_proposal(self.height, lock_round)
                .expect("Can not get lock_proposal, it should not happen!");
            let lock_proposal = lock_signed_proposal.proposal;

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

            let signed_proposal = self.build_signed_proposal(&proposal)?;
            BftMsg::Proposal(rlp::encode(&signed_proposal))
        } else {
            // if is not locked, transmit the cached proposal
            let block = self
                .feed
                .clone()
                .expect("Have checked before, should not happen!");
            let block_hash = self.function.crypt_hash(&block);
            self.block_hash = Some(block_hash.clone());
            info!("Bft is ready to transmit new Proposal");

            let proposal = Proposal {
                height: self.height,
                round: self.round,
                block,
                proof: self.proof.clone(),
                lock_round: None,
                lock_votes: Vec::new(),
                proposer: self.params.address.clone(),
            };

            let signed_proposal = self.build_signed_proposal(&proposal)?;
            BftMsg::Proposal(rlp::encode(&signed_proposal))
        };
        info!(
            "Bft transmits proposal at height {:?}, round {:?}",
            self.height, self.round
        );
        self.function.transmit(msg.clone());
        self.send_bft_msg(msg)?;
        Ok(())
    }

    fn transmit_prevote(&mut self) -> BftResult<()> {
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

        let vote = Vote {
            vote_type: VoteType::Prevote,
            height: self.height,
            round: self.round,
            block_hash: block_hash.clone(),
            voter: self.params.address.clone(),
        };
        let signed_vote = self.build_signed_vote(&vote)?;
        let msg = BftMsg::Vote(rlp::encode(&signed_vote));

        info!("Bft prevotes to {:?}", block_hash);
        self.function.transmit(msg.clone());
        self.send_bft_msg(msg)?;

        self.change_to_step(Step::Prevote);

        self.set_timer(
            self.params.timer.get_prevote() * TIMEOUT_RETRANSE_COEF,
            Step::Prevote,
        );

        Ok(())
    }

    fn transmit_precommit(&mut self) -> BftResult<()> {
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
        let signed_vote = self.build_signed_vote(&vote)?;
        let msg = BftMsg::Vote(rlp::encode(&signed_vote));

        info!("Bft precommits to {:?}", block_hash);
        self.function.transmit(msg.clone());
        self.send_bft_msg(msg)?;

        self.set_timer(
            self.params.timer.get_precommit() * TIMEOUT_RETRANSE_COEF,
            Step::Precommit,
        );
        Ok(())
    }

    fn retransmit_lower_votes(&self, round: u64) -> BftResult<()> {
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
        let signed_prevote = self.build_signed_vote(&prevote)?;
        self.function
            .transmit(BftMsg::Vote(rlp::encode(&signed_prevote)));

        let precommit = Vote {
            vote_type: VoteType::Precommit,
            height: self.height - 1,
            round,
            block_hash: self.last_commit_block_hash.clone().unwrap(),
            voter: self.params.address.clone(),
        };
        let signed_precommit = self.build_signed_vote(&precommit)?;
        self.function
            .transmit(BftMsg::Vote(rlp::encode(&signed_precommit)));
        Ok(())
    }

    fn retransmit_nil_precommit(&self, vote: &Vote) -> BftResult<()> {
        let precommit = Vote {
            vote_type: VoteType::Precommit,
            height: vote.height,
            round: vote.round,
            block_hash: Vec::new(),
            voter: self.params.address.clone(),
        };
        let signed_precommit = self.build_signed_vote(&precommit)?;

        info!("Bft finds some nodes fall behind, and sends nil vote to help them pursue");
        self.function
            .transmit(BftMsg::Vote(rlp::encode(&signed_precommit)));
        Ok(())
    }

    #[inline]
    fn change_to_step(&mut self, step: Step) {
        self.step = step;
    }

    fn new_round_start(&mut self, new_round: bool) -> BftResult<()> {
        if self.step != Step::ProposeWait {
            info!(
                "Bft starts height {:?}, round {:?}",
                self.height, self.round
            );
        }
        self.change_to_step(Step::ProposeWait);

        if self.is_proposer()? {
            if new_round {
                self.clean_feed();
                let function = self.function.clone();
                let sender = self.msg_sender.clone();
                let height = self.height;
//                cross_thread::scope(|s| {
//                    s.spawn(move |_| {
//                        info!("get block req!");
//                        if let Ok(block) = function.get_block(height) {
//                            let feed = Feed { height, block };
//                            info!("get block resp {:?}!", &feed);
//                            sender.send(BftMsg::Feed(feed)).unwrap();
//                        }
//                    });
//                })
//                .unwrap();

                thread::spawn(move || {
                    info!("get block req!");
                    if let Ok(block) = function.get_block(height) {
                        let feed = Feed { height, block };
                        info!("get block resp {:?}!", &feed);
                        sender.send(BftMsg::Feed(feed)).unwrap();
                    }
                });
            }
            info!("ready to transmit proposal!");
            self.transmit_proposal()?;
            self.transmit_prevote()?;
        }

        Ok(())
    }

    #[inline]
    fn goto_new_height(&mut self, new_height: u64) {
        self.clean_save_info();
        self.clean_filter();
        let _ = self.wal_log.set_height(new_height);

        self.height = new_height;
        self.round = 0;

        let now = Instant::now();
        info!(
            "Bft goto new height {}, last height cost {:?} to reach consensus",
            new_height,
            now - self.htime
        );
        self.htime = now;
    }

    #[inline]
    fn goto_next_round(&mut self) {
        info!("Bft goto next round {:?}", self.round + 1);
        self.round_filter.clear();
        self.round += 1;
    }

    fn is_proposer(&self) -> BftResult<bool> {
        let proposer = self.get_proposer(self.height, self.round)?;
        info!(
            "Bft chooses proposer {:?} at height {}, round {}",
            proposer, self.height, self.round
        );

        if self.params.address == *proposer {
            info!(
                "Bft becomes proposer at height {}, round {}",
                self.height, self.round
            );
            return Ok(true);
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
        Ok(false)
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

    pub(crate) fn set_proof(&mut self, proof: &Proof) {
        if self.proof.height < proof.height {
            self.proof = proof.clone();
        }
    }

    fn set_status(&mut self, status: &Status) {
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
    }

    fn set_polc(&mut self, hash: &Hash, voteset: &VoteSet) {
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
                            self.set_polc(hash, &prevote_set);
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
            let round = lock_status.round;
            if self.verify_results.contains_key(&round) {
                if *self.verify_results.get(&round).unwrap() {
                    return VerifyResult::Approved;
                } else {
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

        #[cfg(feature = "verify_req")]
        self.verify_results.clear();
    }

    #[inline]
    fn clean_feed(&mut self) {
        self.feed = None;
    }

    #[inline]
    fn clean_filter(&mut self) {
        self.height_filter.clear();
        self.round_filter.clear();
    }
}
