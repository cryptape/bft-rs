use crate::*;
use crate::{
    collectors::{BlockCollector, ProposalCollector, VoteCollector},
    error::{handle_err, BftError, BftResult},
    objects::*,
    params::BftParams,
    timer::{TimeoutInfo, WaitTimer},
    utils::extract_two,
    wal::Wal,
};

use crossbeam::crossbeam_channel::{select, unbounded, Receiver, RecvError, Sender};
#[allow(unused_imports)]
use log::{debug, error, info, log};
use std::collections::HashMap;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

pub(crate) const INIT_HEIGHT: Height = 0;
pub(crate) const INIT_ROUND: Round = 0;
pub(crate) const PROPOSAL_TIMES_COEF: u64 = 4;
pub(crate) const TIMEOUT_RETRANSE_COEF: u32 = 15;

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
    pub(crate) feed: Option<Hash>,
    pub(crate) status: Option<Status>,
    pub(crate) verify_results: HashMap<Round, VerifyResp>,
    pub(crate) proof: Proof,
    pub(crate) blocks: BlockCollector,
    pub(crate) proposals: ProposalCollector,
    pub(crate) votes: VoteCollector,
    pub(crate) wal_log: Wal,

    // user define
    pub(crate) function: Arc<T>,
    pub(crate) consensus_power: bool,

    // byzantine mark
    pub(crate) is_byzantine: bool,
}

impl<T> Bft<T>
where
    T: BftSupport + 'static,
{
    fn new(
        s: Sender<BftMsg>,
        r: Receiver<BftMsg>,
        ts: Sender<TimeoutInfo>,
        tn: Receiver<TimeoutInfo>,
        f: Arc<T>,
        local_address: Address,
        wal_path: &str,
    ) -> Self {
        info!(
            "Node {:?} initializing with wal_path: {}",
            local_address, wal_path
        );
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
            blocks: BlockCollector::new(),
            proposals: ProposalCollector::new(),
            votes: VoteCollector::new(),
            wal_log: Wal::new(wal_path).unwrap(),
            function: f,
            consensus_power: false,
            is_byzantine: false,
        }
    }

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

        let mut engine = Bft::new(
            s,
            r,
            bft2timer,
            bft4timer,
            f,
            local_address.clone(),
            wal_path,
        );

        // start timer module.
        let _timer_thread = thread::Builder::new()
            .name("bft_timer".to_string())
            .spawn(move || {
                let timer = WaitTimer::new(timer2bft, timer4bft);
                timer.start();
            })
            .unwrap_or_else(|_| panic!("Node {:?} starts time-thread failed!", local_address));

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

                    if let Ok(msg) = get_timer_msg {
                        handle_err(engine.timeout_process(msg, true), &engine.params.address);
                    }
                    if let Ok(msg) = get_msg {
                        match msg {
                            BftMsg::Kill => {
                                break;
                            }
                            _ => {
                                handle_err(engine.process(msg, true), &engine.params.address);
                            }
                        }
                    }
                }
            })
            .unwrap_or_else(|_| panic!("Node {:?} starts main-thread failed!", local_address));
    }

    pub(crate) fn process(&mut self, msg: BftMsg, need_wal: bool) -> BftResult<()> {
        match msg {
            BftMsg::Proposal(encode) => {
                trace!(
                    "Node {:?} has consensus power {:?}",
                    self.params.address,
                    self.consensus_power
                );
                if self.consensus_power {
                    let (signed_proposal_encode, block) = extract_two(&encode)?;
                    let signed_proposal: SignedProposal = rlp::decode(&signed_proposal_encode)
                        .map_err(|e| {
                            BftError::DecodeErr(format!("signed_proposal encounters {:?}", e))
                        })?;
                    debug!(
                        "Node {:?} receives {:?}",
                        self.params.address, &signed_proposal
                    );
                    self.check_and_save_proposal(
                        &signed_proposal,
                        &block.into(),
                        &signed_proposal_encode,
                        need_wal,
                    )?;

                    let proposal = signed_proposal.proposal;
                    if self.step <= Step::ProposeWait {
                        self.handle_proposal(&proposal)?;
                        self.set_proposal(proposal);
                        if self.step == Step::ProposeWait {
                            self.transmit_prevote(false)?;
                        }
                    }
                    // handle commit after proposal is ready while bft process blocked in Commit Step
                    if self.step == Step::Commit {
                        info!(
                            "Node {:?} receives lacking proposal in commit step",
                            self.params.address
                        );
                        self.handle_commit()?;
                    }
                }
            }

            BftMsg::Vote(encode) => {
                if self.consensus_power {
                    let signed_vote: SignedVote = rlp::decode(&encode).map_err(|e| {
                        BftError::DecodeErr(format!("signed_vote encounters {:?}", e))
                    })?;
                    debug!("Node {:?} receives {:?}", self.params.address, signed_vote);
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
                debug!("Node {:?} receives {:?}", self.params.address, &feed);
                self.check_and_save_feed(&feed, need_wal)?;

                if self.step == Step::ProposeWait {
                    self.new_round_start(false)?;
                }
            }

            BftMsg::Status(status) => {
                debug!("Node {:?} receives {:?}", self.params.address, &status);
                self.check_and_save_status(&status, need_wal)?;
                self.handle_status(status)?;
            }

            #[cfg(feature = "verify_req")]
            BftMsg::VerifyResp(verify_resp) => {
                debug!("Node {:?} receives {:?}", self.params.address, &verify_resp);
                self.check_and_save_verify_resp(&verify_resp, need_wal)?;

                if self.step == Step::VerifyWait {
                    if self.check_verify() == VerifyResult::Undetermined {
                        self.change_to_step(Step::VerifyWait);
                    } else {
                        self.transmit_precommit(false)?;
                    }
                }
            }

            BftMsg::Pause => {
                self.consensus_power = false;
                info!("Node {:?} pauses bft process", self.params.address);
            }

            BftMsg::Start => {
                self.consensus_power = true;
                info!("Node {:?} starts bft process", self.params.address);
            }

            BftMsg::Clear(proof) => {
                info!(
                    "Node {:?} receives clear with {:?}",
                    self.params.address, &proof
                );
                self.clear(proof);
            }

            BftMsg::Corrupt => {
                info!("Node {:?} is corrupt to be byzantine", self.params.address);
                self.is_byzantine = true;
            }

            _ => {}
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

        if need_wal && tminfo.step != Step::Prevote && tminfo.step != Step::Precommit {
            handle_err(
                self.wal_log
                    .save(self.height, LogType::TimeOutInfo, &rlp::encode(&tminfo))
                    .or_else(|e| Err(BftError::SaveWalErr(format!("{:?} of {:?}", e, &tminfo)))),
                &self.params.address,
            );
        }

        match tminfo.step {
            Step::ProposeWait => {
                info!(
                    "Node {:?} receives time event Step::ProposeWait",
                    self.params.address
                );
                self.change_to_step(Step::Prevote);
                self.transmit_prevote(false)?;
            }
            Step::Prevote => {
                info!(
                    "Node {:?} receives time event Step::Prevote",
                    self.params.address
                );
                self.transmit_prevote(true)?;
            }
            Step::PrevoteWait => {
                info!(
                    "Node {:?} receives time event Step::PrevoteWait",
                    self.params.address
                );
                // if there is no lock, clear the proposal
                if self.lock_status.is_none() {
                    self.block_hash = None;
                }

                #[cfg(feature = "verify_req")]
                {
                    let verify_result = self.check_verify();
                    if verify_result == VerifyResult::Undetermined {
                        self.change_to_step(Step::VerifyWait);
                        return Ok(());
                    }
                }

                self.transmit_precommit(false)?;
            }
            Step::Precommit => {
                info!(
                    "Node {:?} receives time event Step::Precommit",
                    self.params.address
                );
                handle_err(self.transmit_prevote(true), &self.params.address);
                self.transmit_precommit(true)?;
            }
            Step::PrecommitWait => {
                info!(
                    "Node {:?} receives time event Step::PrecommitWait",
                    self.params.address
                );
                self.goto_next_round();
                self.new_round_start(true)?;
            }

            #[cfg(feature = "verify_req")]
            Step::VerifyWait => {
                info!(
                    "Node {:?} receives time event Step::VerifyWait",
                    self.params.address
                );
                let signed_proposal = self
                    .proposals
                    .get_proposal(self.height, self.round)
                    .ok_or_else(|| {
                        BftError::ShouldNotHappen(
                            "can not fetch proposal from cache when handle commit".to_string(),
                        )
                    })?;
                let proposal = signed_proposal.proposal;
                if proposal.lock_round.is_some() {
                    self.clean_polc();
                }
                self.transmit_precommit(false)?;
            }

            Step::CommitWait => {
                info!(
                    "Node {:?} receives time event Step::CommitWait",
                    self.params.address
                );
                self.set_status(&self.status.clone().unwrap());
                self.goto_new_height(self.height + 1);
                handle_err(self.flush_cache(), &self.params.address);
                self.new_round_start(true)?;
            }
            _ => error!("Invalid Timeout Info!"),
        }

        Ok(())
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
        } else if vote.height == self.height && self.round != 0 && vote.round == self.round - 1 {
            // deal with equal height, round fall behind
            let voter = vote.voter.clone();
            let trans_flag = self.filter_round(&voter);

            if trans_flag {
                self.round_filter.insert(voter, Instant::now());
                self.retransmit_nil_precommit(&vote)?;
            }
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
        let lock_status = self
            .lock_status
            .clone()
            .unwrap_or_else(|| panic!("Node {:?} has no lock when commit!", self.params.address));

        let proof = self.generate_proof(lock_status.clone());
        debug!("generate {:?} when handle commit", proof);
        self.set_proof(&proof, true);

        let signed_proposal = self
            .proposals
            .get_proposal(self.height, self.round)
            .ok_or_else(|| {
                BftError::NotReady(
                    "can not fetch proposal from cache when handle commit".to_string(),
                )
            })?;
        let proposal = signed_proposal.proposal;
        #[cfg(not(feature = "compact_block"))]
        let block = self
            .blocks
            .get_block(self.height, &proposal.block_hash)
            .ok_or_else(|| {
                BftError::ShouldNotHappen("can not fetch block from cache when commit".to_string())
            })?
            .clone();
        #[cfg(feature = "compact_block")]
        let block = self
            .verify_results
            .get(&self.round)
            .ok_or_else(|| {
                BftError::ShouldNotHappen(
                    "can not fetch complete block from cache when commit".to_string(),
                )
            })?
            .clone()
            .complete_block;

        let commit = Commit {
            height: self.height,
            block,
            proof,
            address: proposal.proposer.clone(),
        };

        info!(
            "Node {:?} commits {:?} at height {:?}, consumes consensus time {:?}",
            self.params.address,
            lock_status.block_hash,
            self.height,
            Instant::now() - self.htime
        );

        let function = self.function.clone();
        let sender = self.msg_sender.clone();
        let address = self.params.address.clone();
        thread::spawn(move || {
            handle_err(
                function
                    .commit(commit)
                    .map_err(|e| BftError::CommitFailed(format!("{:?}", e)))
                    .and_then(|status| {
                        sender
                            .send(BftMsg::Status(status))
                            .map_err(|e| BftError::SendMsgErr(format!("{:?}", e)))
                    }),
                &address,
            );
        });

        self.last_commit_round = Some(self.round);
        self.last_commit_block_hash = Some(proposal.block_hash);
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
                    let tv = if cost_time < interval {
                        interval - cost_time
                    } else {
                        Duration::new(0, 0)
                    };
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
            handle_err(self.flush_cache(), &self.params.address);
            self.new_round_start(true)?;

            debug!(
                "Node {:?} receives status, goto new height {:?}",
                self.params.address,
                status.height + 1
            );
            return Ok(());
        }
        Err(BftError::ObsoleteMsg(format!("{:?}", &status)))
    }

    fn transmit_proposal(&mut self) -> BftResult<()> {
        if self.is_byzantine {
            return self.transmit_byzantine_proposal();
        }

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
            debug!(
                "Node {:?} is ready to transmit a locked proposal",
                self.params.address
            );
            let lock_status = self.lock_status.clone().unwrap();
            let lock_round = lock_status.round;
            let lock_votes = lock_status.votes;

            let lock_signed_proposal = self
                .proposals
                .get_proposal(self.height, lock_round)
                .unwrap_or_else(|| {
                    panic!(
                        "Node {:?} can not get lock_proposal, it should not happen!",
                        self.params.address
                    )
                });
            let lock_proposal = lock_signed_proposal.proposal;
            let block_hash = lock_proposal.block_hash;

            let proposal = Proposal {
                height: self.height,
                round: self.round,
                block_hash,
                proof: lock_proposal.proof,
                lock_round: Some(lock_round),
                lock_votes,
                proposer: self.params.address.clone(),
            };
            let encode = self.build_signed_proposal_encode(&proposal)?;
            BftMsg::Proposal(encode)
        } else {
            // if is not locked, transmit the cached proposal
            let block_hash = self.feed.clone().unwrap_or_else(|| {
                panic!(
                    "Node {:?} has checked before, it should not happen!",
                    self.params.address
                )
            });
            self.block_hash = Some(block_hash.clone());
            debug!(
                "Node {:?} is ready to transmit a new proposal",
                self.params.address
            );

            let proposal = Proposal {
                height: self.height,
                round: self.round,
                block_hash,
                proof: self.proof.clone(),
                lock_round: None,
                lock_votes: Vec::new(),
                proposer: self.params.address.clone(),
            };
            let encode = self.build_signed_proposal_encode(&proposal)?;
            BftMsg::Proposal(encode)
        };
        debug!(
            "Node {:?} transmits proposal at h:{}, r:{}",
            self.params.address, self.height, self.round
        );
        self.function.transmit(msg.clone());
        self.send_bft_msg(msg)?;
        Ok(())
    }

    pub(crate) fn transmit_prevote(&mut self, resend: bool) -> BftResult<()> {
        if self.is_byzantine {
            return self.transmit_byzantine_prevote(resend);
        }

        let block_hash = if let Some(lock_status) = self.lock_status.clone() {
            lock_status.block_hash
        } else if let Some(block_hash) = self.block_hash.clone() {
            block_hash
        } else {
            Hash::default()
        };

        let vote = Vote {
            vote_type: VoteType::Prevote,
            height: self.height,
            round: self.round,
            block_hash: block_hash.clone(),
            voter: self.params.address.clone(),
        };
        let signed_vote = self.build_signed_vote(&vote)?;
        let msg = BftMsg::Vote(rlp::encode(&signed_vote));

        debug!(
            "Node {:?} prevotes to {:?} at h:{} r:{}",
            self.params.address, block_hash, self.height, self.round
        );
        self.function.transmit(msg.clone());
        if !resend {
            self.change_to_step(Step::Prevote);
            handle_err(self.send_bft_msg(msg), &self.params.address);
        }

        self.set_timer(
            self.params.timer.get_prevote() * TIMEOUT_RETRANSE_COEF,
            Step::Prevote,
        );

        Ok(())
    }

    fn transmit_precommit(&mut self, resend: bool) -> BftResult<()> {
        if self.is_byzantine {
            return self.transmit_byzantine_precommit(resend);
        }

        let block_hash = if let Some(lock_status) = self.lock_status.clone() {
            lock_status.block_hash
        } else {
            self.block_hash = None;
            Hash::default()
        };

        let vote = Vote {
            vote_type: VoteType::Precommit,
            height: self.height,
            round: self.round,
            block_hash: block_hash.clone(),
            voter: self.params.address.clone(),
        };
        let signed_vote = self.build_signed_vote(&vote)?;
        let msg = BftMsg::Vote(rlp::encode(&signed_vote));

        debug!(
            "Node {:?} precommits to {:?} at h:{:?}, r:{:?}",
            self.params.address, block_hash, self.height, self.round
        );
        self.function.transmit(msg.clone());
        if !resend {
            self.change_to_step(Step::Precommit);
            handle_err(self.send_bft_msg(msg), &self.params.address);
        }

        self.set_timer(
            self.params.timer.get_precommit() * TIMEOUT_RETRANSE_COEF,
            Step::Precommit,
        );
        Ok(())
    }

    fn retransmit_lower_votes(&self, round: Round) -> BftResult<()> {
        if self.is_byzantine {
            return self.retransmit_byzantine_lower_votes();
        }

        debug!(
            "Node {:?} receives msg in lower height, retransmit votes",
            self.params.address
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
        if self.is_byzantine {
            return self.retransmit_byzantine_nil_precommit();
        }

        let precommit = Vote {
            vote_type: VoteType::Precommit,
            height: vote.height,
            round: vote.round,
            block_hash: Hash::default(),
            voter: self.params.address.clone(),
        };
        let signed_precommit = self.build_signed_vote(&precommit)?;

        debug!(
            "Node {:?} receives vote in lower round, retransmit nil precommit",
            self.params.address
        );
        self.function
            .transmit(BftMsg::Vote(rlp::encode(&signed_precommit)));
        Ok(())
    }

    fn new_round_start(&mut self, new_round: bool) -> BftResult<()> {
        if self.step != Step::ProposeWait {
            info!(
                "Node {:?} starts h:{}, r:{}, s: {:?}",
                self.params.address, self.height, self.round, self.step
            );
        }
        self.change_to_step(Step::ProposeWait);

        if self.is_proposer()? {
            if new_round {
                self.clean_feed();
                let function = self.function.clone();
                let sender = self.msg_sender.clone();
                let height = self.height;
                let address = self.params.address.clone();
                let proof = self.proof.clone();

                thread::spawn(move || {
                    handle_err(
                        function
                            .get_block(height, &proof)
                            .map_err(|e| BftError::GetBlockFailed(format!("{:?}", e)))
                            .and_then(|(block, block_hash)| {
                                sender
                                    .send(BftMsg::Feed(Feed {
                                        height,
                                        block,
                                        block_hash,
                                    }))
                                    .map_err(|e| BftError::SendMsgErr(format!("{:?}", e)))
                            }),
                        &address,
                    );
                });
            }
            self.transmit_proposal()?;
            self.transmit_prevote(false)?;
        }

        Ok(())
    }

    #[inline]
    fn goto_new_height(&mut self, new_height: Height) {
        self.clean_save_info();
        self.clean_filter();
        handle_err(
            self.wal_log
                .set_height(new_height)
                .map_err(|e| BftError::SaveWalErr(format!("{:?} of set_height", e))),
            &self.params.address,
        );

        self.height = new_height;
        self.round = 0;

        let now = Instant::now();
        info!(
            "Node {:?} goto new height {}, last height costs {:?} to reach consensus",
            self.params.address,
            new_height,
            now - self.htime
        );
        self.htime = now;
    }

    #[inline]
    fn goto_next_round(&mut self) {
        self.round_filter.clear();
        self.round += 1;
        handle_err(
            self.fetch_proposal(self.height, self.round),
            &self.params.address,
        );
    }

    fn is_proposer(&self) -> BftResult<bool> {
        let proposer = self.get_proposer(self.height, self.round)?;
        debug!(
            "Node {:?} chooses proposer {:?} at h:{}, r:{}",
            self.params.address, proposer, self.height, self.round
        );

        if self.params.address == *proposer {
            debug!(
                "Node {:?} becomes proposer at h:{}, r:{}",
                self.params.address, self.height, self.round
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

    fn set_proposal(&mut self, proposal: Proposal) {
        let block_hash = proposal.block_hash;

        if proposal.lock_round.is_some()
            && (self.lock_status.is_none()
                || self.lock_status.clone().unwrap().round <= proposal.lock_round.unwrap())
        {
            // receive a proposal with a later PoLC
            debug!(
                "Node {:?} handles a proposal with a PoLC",
                self.params.address
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
                "Node {:?} handles a proposal without a PoLC",
                self.params.address
            );
            self.block_hash = Some(block_hash);
        } else {
            debug!(
                "Node {:?} handles a proposal with an earlier PoLC",
                self.params.address
            );
        }
    }

    fn check_prevote_count(&mut self) -> bool {
        let mut flag = false;
        for (round, prevote_count) in self.votes.prevote_count.iter() {
            debug!(
                "Node {:?} received {} prevotes in r:{}",
                self.params.address, prevote_count, round
            );
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
                        if hash.0.is_empty() {
                            // receive +2/3 prevote to nil, clean lock info
                            debug!(
                                "Node {:?} collects over 2/3 prevotes on nil at h:{}, r:{}",
                                self.params.address, self.height, self.round
                            );
                            self.clean_polc();
                            self.block_hash = None;
                        } else {
                            // receive a later PoLC, update lock info
                            self.set_polc(hash, &prevote_set);
                        }
                    }
                    if self.lock_status.is_none() && !hash.0.is_empty() {
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
        let mut flag = false;
        for (round, precommit_count) in self.votes.precommit_count.iter() {
            debug!(
                "Node {:?} received {} precommits in r:{}",
                self.params.address, precommit_count, round
            );
            if self.cal_above_threshold(*precommit_count) && *round >= self.round {
                flag = true;
                if self.round < *round {
                    self.round_filter.clear();
                    self.round = *round;
                }
            }
        }
        if !flag {
            return PrecommitRes::Below;
        }

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

            for (hash, count) in &precommit_set.votes_by_proposal {
                if self.cal_above_threshold(*count) {
                    if hash.0.is_empty() {
                        debug!(
                            "Node {:?} reaches nil consensus, goto next round {:?}",
                            self.params.address,
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
                if self.verify_results.get(&round).unwrap().is_pass {
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
}
