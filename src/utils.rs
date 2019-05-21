use crate::collectors::ProposalRoundCollector;
use crate::*;
use crate::{
    algorithm::{Bft, INIT_HEIGHT, INIT_ROUND},
    collectors::{ProposalCollector, RoundCollector, VoteCollector, VoteSet, CACHE_N},
    error::{handle_err, BftError, BftResult},
    objects::*,
    random::get_index,
    timer::TimeoutInfo,
    wal::Wal,
};
#[cfg(feature = "verify_req")]
use std::collections::{HashMap, HashSet};
use std::fs;
use std::thread;
use std::time::{Duration, Instant};

use log::warn;

const TIMEOUT_LOW_HEIGHT_MESSAGE_COEF: u32 = 20;
const TIMEOUT_LOW_ROUND_MESSAGE_COEF: u32 = 20;

impl<T> Bft<T>
where
    T: BftSupport + 'static,
{
    pub(crate) fn load_wal_log(&mut self) {
        // TODO: should prevent saving wal
        info!("Node {:?} starts to load wal log!", self.params.address);
        let vec_buf = self.wal_log.load();
        for (log_type, encode) in vec_buf {
            handle_err(self.process_wal_log(log_type, encode));
        }
        info!(
            "Node {:?} successfully processes the whole wal log!",
            self.params.address
        );
    }

    fn process_wal_log(&mut self, log_type: LogType, encode: Vec<u8>) -> BftResult<()> {
        match log_type {
            LogType::Proposal => {
                info!("Node {:?} load proposal", self.params.address);
                let signed_proposal: SignedProposal = rlp::decode(&encode).map_err(|e| {
                    BftError::DecodeErr(format!("signed_proposal encounters {:?}", e))
                })?;
                let proposal = signed_proposal.proposal;
                let block_hash = proposal.block_hash;
                let block = self
                    .blocks
                    .get_block(proposal.height, &block_hash)
                    .ok_or_else(|| {
                        BftError::ShouldNotHappen(
                            "can not fetch block from cache when load signed_proposal".to_string(),
                        )
                    })?;
                let proposal_block_encode = combine_two(&encode, block);
                self.process(BftMsg::Proposal(proposal_block_encode), false)?;
            }
            LogType::Vote => {
                info!("Node {:?} load vote", self.params.address);
                self.process(BftMsg::Vote(encode), false)?;
            }
            LogType::Feed => {
                info!("Node {:?} load feed", self.params.address);
                let feed: Feed = rlp::decode(&encode)
                    .map_err(|e| BftError::DecodeErr(format!("feed encounters {:?}", e)))?;
                self.process(BftMsg::Feed(feed), false)?;
            }
            LogType::Status => {
                info!("Node {:?} load status", self.params.address);
                let status: Status = rlp::decode(&encode)
                    .map_err(|e| BftError::DecodeErr(format!("status encounters {:?}", e)))?;
                self.process(BftMsg::Status(status), false)?;
            }
            LogType::Proof => {
                info!("Node {:?} load proof", self.params.address);
                let proof: Proof = rlp::decode(&encode)
                    .map_err(|e| BftError::DecodeErr(format!("proof encounters {:?}", e)))?;
                self.proof = proof;
            }
            #[cfg(feature = "verify_req")]
            LogType::VerifyResp => {
                info!("Node {:?} load verify_resp", self.params.address);
                let verify_resp: VerifyResp = rlp::decode(&encode)
                    .map_err(|e| BftError::DecodeErr(format!("verify_resp encounters {:?}", e)))?;
                self.process(BftMsg::VerifyResp(verify_resp), false)?;
            }

            LogType::TimeOutInfo => {
                info!("Node {:?} load timeout_info", self.params.address);
                let time_out_info: TimeoutInfo = rlp::decode(&encode).map_err(|e| {
                    BftError::DecodeErr(format!("time_out_info encounters {:?}", e))
                })?;
                self.timeout_process(time_out_info, false)?;
            }

            LogType::Block => {
                info!("Node {:?} load block", self.params.address);
                let (height, block, block_hash) = decode_block(&encode)?;
                self.blocks.add(height, block_hash, block);
            }
        }
        Ok(())
    }

    pub(crate) fn build_signed_proposal_encode(
        &mut self,
        proposal: &Proposal,
    ) -> BftResult<Vec<u8>> {
        let block_hash = &proposal.block_hash;
        let signed_proposal = self.build_signed_proposal(&proposal)?;
        let signed_proposal_encode = rlp::encode(&signed_proposal);
        let block = self
            .blocks
            .get_block(proposal.height, block_hash)
            .ok_or_else(|| {
                BftError::ShouldNotHappen(
                    "can not fetch block from cache when send signed_proposal".to_string(),
                )
            })?;
        let encode = combine_two(&signed_proposal_encode, block);
        Ok(encode)
    }

    pub(crate) fn build_signed_proposal(&self, proposal: &Proposal) -> BftResult<SignedProposal> {
        let encode = rlp::encode(proposal);
        let hash = self.function.crypt_hash(&encode);

        let signature = self
            .function
            .sign(&hash)
            .map_err(|e| BftError::SignFailed(format!("{:?} of {:?}", e, proposal)))?;

        Ok(SignedProposal {
            proposal: proposal.clone(),
            signature,
        })
    }

    pub(crate) fn build_signed_vote(&self, vote: &Vote) -> BftResult<SignedVote> {
        let encode = rlp::encode(vote);
        let hash = self.function.crypt_hash(&encode);

        let signature = self
            .function
            .sign(&hash)
            .map_err(|e| BftError::SignFailed(format!("{:?} of {:?}", e, vote)))?;

        Ok(SignedVote {
            vote: vote.clone(),
            signature,
        })
    }

    #[inline]
    fn get_authorities(&self, height: u64) -> BftResult<&Vec<Node>> {
        let p = &self.authority_manage;
        let authorities = if height == p.authority_h_old {
            &p.authorities_old
        } else {
            &p.authorities
        };

        if authorities.is_empty() {
            return Err(BftError::ShouldNotHappen(
                "the authority_list is empty".to_string(),
            ));
        }
        Ok(authorities)
    }

    #[inline]
    fn get_vote_weight(&self, height: u64, address: &[u8]) -> u64 {
        if height != self.height {
            return 1u64;
        }
        if let Some(node) = self
            .authority_manage
            .authorities
            .iter()
            .find(|node| node.address == address.to_vec())
        {
            return u64::from(node.vote_weight);
        }
        1u64
    }

    pub(crate) fn get_proposer(&self, height: u64, round: u64) -> BftResult<&Address> {
        let authorities = self.get_authorities(height)?;
        let nonce = height + round;
        let weight: Vec<u64> = authorities
            .iter()
            .map(|node| u64::from(node.proposal_weight))
            .collect();
        let proposer: &Address = &authorities
            .get(get_index(nonce, &weight))
            .unwrap_or_else(|| {
                panic!(
                    "Node {:?} selects a proposer not in authorities, it should not happen!",
                    self.params.address
                )
            })
            .address;
        Ok(proposer)
    }

    #[inline]
    pub(crate) fn set_proof(&mut self, proof: &Proof) {
        if self.proof.height < proof.height {
            self.proof = proof.clone();
        }
    }

    pub(crate) fn set_status(&mut self, status: &Status) {
        self.authority_manage
            .receive_authorities_list(status.height, status.authority_list.clone());
        trace!(
            "Node {:?} updates authority_manage {:?}",
            self.params.address,
            self.authority_manage
        );

        if self.consensus_power
            && !status
                .authority_list
                .iter()
                .any(|node| node.address == self.params.address)
        {
            info!(
                "Node {:?} loses consensus power in height {} and stops the bft-rs process!",
                self.params.address, status.height
            );
            self.consensus_power = false;
        } else if !self.consensus_power
            && status
                .authority_list
                .iter()
                .any(|node| node.address == self.params.address)
        {
            info!(
                "Node {:?} accesses consensus power in height {} and starts the bft-rs process!",
                self.params.address, status.height
            );
            self.consensus_power = true;
        }

        if let Some(interval) = status.interval {
            // update the bft interval
            self.params.timer.set_total_duration(interval);
        }
    }

    pub(crate) fn set_polc(&mut self, hash: &[u8], voteset: &VoteSet) {
        self.block_hash = Some(hash.to_owned());
        self.lock_status = Some(LockStatus {
            block_hash: hash.to_owned(),
            round: self.round,
            votes: voteset.extract_polc(hash),
        });

        debug!(
            "Node {:?} sets a PoLC at height {:?}, round {:?}, on proposal {:?}",
            self.params.address,
            self.height,
            self.round,
            hash.to_owned()
        );
    }

    #[inline]
    pub(crate) fn set_timer(&self, duration: Duration, step: Step) {
        debug!(
            "Node {:?} sets {:?} timer for {:?}",
            self.params.address, step, duration
        );
        let timestamp = Instant::now() + duration;
        let since = timestamp - self.htime;
        self.timer_seter
            .send(TimeoutInfo {
                timestamp,
                duration: since.as_nanos() as u64,
                height: self.height,
                round: self.round,
                step,
            })
            .unwrap();
    }

    pub(crate) fn generate_proof(&mut self, lock_status: LockStatus) -> Proof {
        let block_hash = lock_status.block_hash;
        let lock_votes = lock_status.votes;
        let precommit_votes: HashMap<Address, Signature> = lock_votes
            .into_iter()
            .map(|signed_vote| (signed_vote.vote.voter, signed_vote.signature))
            .collect();
        Proof {
            height: self.height,
            round: lock_status.round,
            block_hash,
            precommit_votes,
        }
    }

    pub(crate) fn flush_cache(&mut self) -> BftResult<()> {
        let proposals = &mut self.proposals.proposals;
        let round_proposals = proposals.get_mut(&self.height);
        let mut proposal_collector = ProposalRoundCollector::new();

        let votes = &mut self.votes.votes;
        let round_votes = votes.get_mut(&self.height);
        let mut vote_collector = RoundCollector::new();

        if round_proposals.is_some() {
            proposal_collector = round_proposals.unwrap().clone();
            self.proposals.remove(self.height);
        }

        if round_votes.is_some() {
            vote_collector = round_votes.unwrap().clone();
            self.votes.remove(self.height);
        }

        for (_, signed_proposal) in proposal_collector.round_proposals.iter() {
            let block_hash = signed_proposal.proposal.block_hash.clone();
            let block = self
                .blocks
                .get_block(self.height, &block_hash)
                .ok_or_else(|| {
                    BftError::ShouldNotHappen(
                        "can not fetch block from cache when load signed_proposal".to_string(),
                    )
                })?;
            let proposal_encode = rlp::encode(signed_proposal);
            let encode = combine_two(&proposal_encode, block);
            let msg = BftMsg::Proposal(encode);
            let info = format!("{:?}", &msg);
            self.msg_sender
                .send(msg)
                .map_err(|_| BftError::SendMsgErr(info))?;
        }

        for (_, step_votes) in vote_collector.round_votes.iter() {
            for (_, vote_set) in step_votes.step_votes.iter() {
                for (_, signed_vote) in vote_set.votes_by_sender.iter() {
                    let encode = rlp::encode(signed_vote);
                    let msg = BftMsg::Vote(encode);
                    let info = format!("{:?}", &msg);
                    self.msg_sender
                        .send(msg)
                        .map_err(|_| BftError::SendMsgErr(info))?;
                }
            }
        }

        Ok(())
    }

    #[cfg(feature = "verify_req")]
    pub(crate) fn save_verify_res(&mut self, round: Round, is_pass: bool) -> BftResult<()> {
        if self.verify_results.contains_key(&round) {
            if is_pass != *self.verify_results.get(&round).unwrap() {
                return Err(BftError::ShouldNotHappen(format!(
                    "get conflict verify result of round: {}",
                    round
                )));
            }
        }
        self.verify_results.entry(round).or_insert(is_pass);
        Ok(())
    }

    pub(crate) fn check_and_save_proposal(
        &mut self,
        signed_proposal: &SignedProposal,
        block: &[u8],
        encode: &[u8],
        need_wal: bool,
    ) -> BftResult<()> {
        debug!(
            "Node {:?} receives {:?}",
            self.params.address, signed_proposal
        );
        let proposal = &signed_proposal.proposal;
        let block_hash = &proposal.block_hash;
        let height = proposal.height;
        let round = proposal.round;

        if height < self.height - 1 {
            return Err(BftError::ObsoleteMsg(format!("{:?}", signed_proposal)));
        }

        let address = self
            .function
            .check_sig(
                &signed_proposal.signature,
                &self.function.crypt_hash(&rlp::encode(proposal)),
            )
            .map_err(|e| BftError::CheckSigFailed(format!("{:?} of {:?}", e, signed_proposal)))?;
        if proposal.proposer != address {
            return Err(BftError::InvalidSender(format!(
                "recovers {:?} of {:?}",
                address, signed_proposal
            )));
        }

        // prevent too many higher proposals flush out current proposal
        // because self.height - 1 is allowed, so judge height < self.height + CACHE_N - 1
        if height < self.height + CACHE_N - 1 && round < self.round + CACHE_N {
            self.proposals.add(&signed_proposal)?;
            let save = self.blocks.add(height, block_hash, block);

            if need_wal {
                if save {
                    let encode = encode_block(height, block, block_hash);
                    handle_err(
                        self.wal_log
                            .save(height, LogType::Block, &encode)
                            .or_else(|e| {
                                Err(BftError::SaveWalErr(format!(
                                    "{:?} of proposal block with height {}, round {}",
                                    e, height, round
                                )))
                            }),
                    );
                }
                handle_err(
                    self.wal_log
                        .save(height, LogType::Proposal, &rlp::encode(signed_proposal))
                        .or_else(|e| {
                            Err(BftError::SaveWalErr(format!(
                                "{:?} of {:?}",
                                e, signed_proposal
                            )))
                        }),
                );
            }
        }

        if height > self.height || (height == self.height && round >= self.round + CACHE_N) {
            return Err(BftError::HigherMsg(format!("{:?}", signed_proposal)));
        }

        self.check_proposer(proposal)?;
        self.check_lock_votes(proposal, block_hash)?;

        if height == self.height - 1 {
            return Ok(());
        }
        self.check_block_txs(proposal, block, &self.function.crypt_hash(encode))?;
        self.check_proof(height, &proposal.proof)?;

        Ok(())
    }

    pub(crate) fn check_and_save_vote(
        &mut self,
        signed_vote: &SignedVote,
        need_wal: bool,
    ) -> BftResult<()> {
        debug!("Node {:?} receives {:?}", self.params.address, signed_vote);
        let vote = &signed_vote.vote;
        let height = vote.height;
        let round = vote.round;
        if height < self.height - 1 {
            return Err(BftError::ObsoleteMsg(format!("{:?}", signed_vote)));
        }

        let vote_hash = self.function.crypt_hash(&rlp::encode(vote));
        let address = self
            .function
            .check_sig(&signed_vote.signature, &vote_hash)
            .map_err(|e| BftError::CheckSigFailed(format!("{:?} of {:?}", e, signed_vote)))?;
        if vote.voter != address {
            return Err(BftError::InvalidSender(format!(
                "recovers {:?} of {:?}",
                address, signed_vote
            )));
        }

        // prevent too many high proposals flush out current proposal
        if height < self.height + CACHE_N && round < self.round + CACHE_N {
            let vote_weight = self.get_vote_weight(vote.height, &vote.voter);
            let result = self.votes.add(&signed_vote, vote_weight, self.height);
            if need_wal && result.is_ok() {
                handle_err(
                    self.wal_log
                        .save(height, LogType::Vote, &rlp::encode(signed_vote))
                        .or_else(|e| {
                            Err(BftError::SaveWalErr(format!(
                                "{:?} of {:?}",
                                e, signed_vote
                            )))
                        }),
                );
            }
            handle_err(result);
        }

        if height > self.height || round >= self.round + CACHE_N {
            return Err(BftError::HigherMsg(format!("{:?}", signed_vote)));
        }

        self.check_voter(vote)?;

        Ok(())
    }

    pub(crate) fn check_and_save_status(
        &mut self,
        status: &Status,
        need_wal: bool,
    ) -> BftResult<()> {
        let height = status.height;
        if self.height > 0 && height < self.height - 1 {
            return Err(BftError::ObsoleteMsg(format!("{:?}", status)));
        }
        if need_wal {
            handle_err(
                self.wal_log
                    .save(self.height + 1, LogType::Proof, &rlp::encode(&self.proof))
                    .or_else(|e| {
                        Err(BftError::SaveWalErr(format!(
                            "{:?} of {:?}",
                            e, &self.proof
                        )))
                    }),
            );
            let status_height = status.height;
            handle_err(
                self.wal_log
                    .save(status_height + 1, LogType::Status, &rlp::encode(status))
                    .or_else(|e| Err(BftError::SaveWalErr(format!("{:?} of {:?}", e, status)))),
            );
        }

        Ok(())
    }

    #[cfg(feature = "verify_req")]
    pub(crate) fn check_and_save_verify_resp(
        &mut self,
        verify_resp: &VerifyResp,
        need_wal: bool,
    ) -> BftResult<()> {
        if need_wal {
            handle_err(
                self.wal_log
                    .save(self.height, LogType::VerifyResp, &rlp::encode(verify_resp))
                    .or(Err(BftError::SaveWalErr(format!("{:?}", verify_resp)))),
            );
        }
        self.save_verify_res(verify_resp.round, verify_resp.is_pass)?;

        Ok(())
    }

    pub(crate) fn check_and_save_feed(&mut self, feed: &Feed, need_wal: bool) -> BftResult<()> {
        let height = feed.height;
        if height < self.height {
            return Err(BftError::ObsoleteMsg(format!(
                "feed with height {}",
                height
            )));
        }

        if height > self.height {
            return Err(BftError::HigherMsg(format!("feed with height {}", height)));
        }

        if need_wal {
            handle_err(
                self.wal_log
                    .save(height, LogType::Feed, &rlp::encode(feed))
                    .or_else(|e| {
                        Err(BftError::SaveWalErr(format!(
                            "{:?} of feed with height {}",
                            e, height
                        )))
                    }),
            );
        }

        let block_hash = feed.block_hash.clone();
        self.blocks.add(height, &block_hash, &feed.block);
        self.feed = Some(block_hash);
        Ok(())
    }

    pub(crate) fn check_block_txs(
        &mut self,
        proposal: &Proposal,
        block: &[u8],
        proposal_hash: &[u8],
    ) -> BftResult<()> {
        let height = proposal.height;
        let round = proposal.round;
        let block_hash = &proposal.block_hash;

        #[cfg(not(feature = "verify_req"))]
        {
            self.function
                .check_block(block, block_hash, height)
                .map_err(|e| BftError::CheckBlockFailed(format!("{:?} of {:?}", e, proposal)))?;
            self.function
                .check_txs(block, block_hash, proposal_hash, height, round)
                .map_err(|e| BftError::CheckTxFailed(format!("{:?} of {:?}", e, proposal)))?;
            Ok(())
        }

        #[cfg(feature = "verify_req")]
        {
            if let Err(e) = self.function.check_block(block, block_hash, height) {
                self.save_verify_res(round, false)?;
                return Err(BftError::CheckBlockFailed(format!(
                    "{:?} of {:?}",
                    e, proposal
                )));
            }

            let function = self.function.clone();
            let sender = self.msg_sender.clone();
            let block = block.to_vec();
            let block_hash = block_hash.clone();
            let proposal_hash = proposal_hash.to_owned();
            let address = self.params.address.clone();
            thread::spawn(move || {
                let is_pass =
                    match function.check_txs(&block, &block_hash, &proposal_hash, height, round) {
                        Ok(_) => true,
                        Err(e) => {
                            warn!(
                                "Node {:?} encounters BftError::CheckTxsFailed({:?})",
                                address, e
                            );
                            false
                        }
                    };
                let verify_resp = VerifyResp { is_pass, round };
                handle_err(
                    sender
                        .send(BftMsg::VerifyResp(verify_resp))
                        .map_err(|e| BftError::SendMsgErr(format!("{:?}", e))),
                );
            });

            Ok(())
        }
    }

    pub(crate) fn check_proof(&mut self, height: u64, proof: &Proof) -> BftResult<()> {
        if height != self.height {
            return Err(BftError::ShouldNotHappen(format!(
                "check_proof for {:?}",
                proof
            )));
        }

        let authorities = self.get_authorities(height)?;
        self.check_proof_only(proof, height, authorities)?;
        self.set_proof(proof);

        Ok(())
    }

    pub(crate) fn check_proof_only(
        &self,
        proof: &Proof,
        height: u64,
        authorities: &[Node],
    ) -> BftResult<()> {
        if proof.height == 0 {
            return Ok(());
        }
        if height != proof.height + 1 {
            return Err(BftError::CheckProofFailed(format!(
                "the height {} is mismatching with proof.height {}",
                height, proof.height
            )));
        }

        let vote_addresses: Vec<Address> = proof
            .precommit_votes
            .iter()
            .map(|(voter, _)| voter.clone())
            .collect();

        if get_votes_weight(authorities, &vote_addresses) * 3 <= get_total_weight(authorities) * 2 {
            return Err(BftError::CheckProofFailed(format!(
                "the proof doesn't collect 2/3+ weight \n {:?} ",
                proof
            )));
        }

        let authority_addresses: Vec<Address> = authorities
            .iter()
            .map(|node| node.address.clone())
            .collect();
        let mut set = HashSet::new();
        for (voter, sig) in proof.precommit_votes.clone() {
            // check repeat
            if !set.insert(voter.clone()) {
                return Err(BftError::CheckProofFailed(format!(
                    "the proof contains repeat votes from the same voter {:?} \n {:?}",
                    &voter, proof
                )));
            }

            if authority_addresses.contains(&voter) {
                let vote = Vote {
                    vote_type: VoteType::Precommit,
                    height: proof.height,
                    round: proof.round,
                    block_hash: proof.block_hash.clone(),
                    voter: voter.clone(),
                };
                let msg = rlp::encode(&vote);
                let address = self
                    .function
                    .check_sig(&sig, &self.function.crypt_hash(&msg))
                    .map_err(|e| {
                        BftError::CheckProofFailed(format!("{:?}, sig {:?} in {:?}", e, sig, proof))
                    })?;
                if address != voter {
                    return Err(BftError::CheckProofFailed(format!(
                        "recover {:?} by voter {:?} in {:?}",
                        &address, &voter, proof
                    )));
                }
            } else {
                return Err(BftError::CheckProofFailed(format!(
                    "voter {:?} invalid in {:?}",
                    &voter, proof
                )));
            }
        }

        Ok(())
    }

    pub(crate) fn check_lock_votes(
        &mut self,
        proposal: &Proposal,
        block_hash: &[u8],
    ) -> BftResult<()> {
        let height = proposal.height;
        if height < self.height - 1 || height > self.height {
            return Err(BftError::ShouldNotHappen(format!(
                "check_lock_votes for {:?}",
                proposal
            )));
        }

        let mut map = HashMap::new();
        if let Some(lock_round) = proposal.lock_round {
            for signed_vote in &proposal.lock_votes {
                let voter = self.check_vote(height, lock_round, block_hash, signed_vote)?;
                if map.insert(voter, 1).is_some() {
                    return Err(BftError::CheckLockVotesFailed(format!(
                        "vote repeat of {:?} in {:?} with lock_votes {:?}",
                        signed_vote, proposal, &proposal.lock_votes
                    )));
                }
            }
        } else {
            return Ok(());
        }

        let authorities = self.get_authorities(height)?;
        let vote_addresses: Vec<Address> = proposal
            .lock_votes
            .iter()
            .map(|signed_vote| signed_vote.vote.voter.clone())
            .collect();

        if get_votes_weight(authorities, &vote_addresses) * 3 > get_total_weight(authorities) * 2 {
            return Ok(());
        }
        Err(BftError::CheckLockVotesFailed(format!(
            "less than 2/3+ weight of {:?} with lock_votes {:?}",
            proposal, &proposal.lock_votes
        )))
    }

    pub(crate) fn check_vote(
        &mut self,
        height: Height,
        round: Round,
        block_hash: &[u8],
        signed_vote: &SignedVote,
    ) -> BftResult<Address> {
        if height < self.height - 1 {
            return Err(BftError::ShouldNotHappen(format!(
                "check_vote for {:?}",
                signed_vote
            )));
        }

        let vote = &signed_vote.vote;
        if vote.height != height || vote.round != round {
            return Err(BftError::CheckLockVotesFailed(format!(
                "vote {:?} mismatching height: {} or round: {}",
                signed_vote, height, round
            )));
        }

        if vote.block_hash != block_hash.to_vec() {
            return Err(BftError::CheckLockVotesFailed(format!(
                "vote {:?} not for rightful block_hash {:?}",
                vote, block_hash
            )));
        }

        let authorities = self.get_authorities(height)?;
        let voter = &vote.voter;
        if !authorities.iter().any(|node| &node.address == voter) {
            return Err(BftError::CheckLockVotesFailed(format!(
                "the voter {:?} not in authorities",
                voter
            )));
        }

        let signature = &signed_vote.signature;
        let vote_hash = self.function.crypt_hash(&rlp::encode(vote));
        let address = self
            .function
            .check_sig(signature, &vote_hash)
            .map_err(|e| {
                BftError::CheckLockVotesFailed(format!(
                    "check sig failed with {:?} of {:?}",
                    e, signed_vote
                ))
            })?;
        if &address != voter {
            return Err(BftError::CheckLockVotesFailed(format!(
                "recover {:?} of {:?}",
                &address, signed_vote
            )));
        }

        let vote_weight = self.get_vote_weight(height, &voter);
        let _ = self.votes.add(&signed_vote, vote_weight, self.height);
        Ok(address)
    }

    pub(crate) fn check_proposer(&self, proposal: &Proposal) -> BftResult<()> {
        let height = proposal.height;
        let round = proposal.round;
        let address = &proposal.proposer;

        if height < self.height - 1 || height > self.height {
            return Err(BftError::ShouldNotHappen(format!(
                "check_proposer for {:?}",
                proposal
            )));
        }
        let proposer = self.get_proposer(height, round)?;
        if proposer == address {
            Ok(())
        } else {
            Err(BftError::InvalidSender(format!(
                "the rightful proposer is {:?} by {:?}",
                proposer, proposal
            )))
        }
    }

    pub(crate) fn check_voter(&self, vote: &Vote) -> BftResult<()> {
        let height = vote.height;
        let voter = &vote.voter;

        if height < self.height - 1 || height > self.height {
            return Err(BftError::ShouldNotHappen(format!(
                "check_voter for {:?}",
                vote
            )));
        }

        let authorities = self.get_authorities(height)?;

        if !authorities.iter().any(|node| node.address == *voter) {
            return Err(BftError::InvalidSender(format!(
                "the {:?} of {:?} not in authorities",
                voter, vote
            )));
        }

        Ok(())
    }

    pub(crate) fn filter_height(&self, voter: &[u8]) -> bool {
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

    pub(crate) fn filter_round(&self, voter: &[u8]) -> bool {
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

    #[inline]
    pub(crate) fn send_bft_msg(&self, msg: BftMsg) -> BftResult<()> {
        let info = format!("{:?}", &msg);
        self.msg_sender
            .send(msg)
            .map_err(|e| BftError::SendMsgErr(format!("{:?} of {:?}", e, info)))
    }

    #[inline]
    pub(crate) fn change_to_step(&mut self, step: Step) {
        self.step = step;
    }

    #[inline]
    pub(crate) fn cal_all_vote(&self, count: u64) -> bool {
        let weight_sum = get_total_weight(&self.authority_manage.authorities);
        count == weight_sum
    }

    #[inline]
    pub(crate) fn cal_above_threshold(&self, count: u64) -> bool {
        let weight_sum = get_total_weight(&self.authority_manage.authorities);
        count * 3 > weight_sum * 2
    }

    pub(crate) fn clean_polc(&mut self) {
        self.block_hash = None;
        self.lock_status = None;
        debug!(
            "Node {:?} cleans PoLC at height {:?}, round {:?}",
            self.params.address, self.height, self.round
        );
    }

    #[inline]
    pub(crate) fn clean_save_info(&mut self) {
        // clear prevote count needed when goto new height
        self.block_hash = None;
        self.lock_status = None;
        self.votes.clear_vote_count();

        #[cfg(feature = "verify_req")]
        self.verify_results.clear();
    }

    #[inline]
    pub(crate) fn clean_feed(&mut self) {
        self.feed = None;
    }

    #[inline]
    pub(crate) fn clean_filter(&mut self) {
        self.height_filter.clear();
        self.round_filter.clear();
    }

    pub(crate) fn clear(&mut self, proof: Proof) {
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
        self.feed = None;
        self.verify_results.clear();
        self.proof = proof;
        self.authority_manage = AuthorityManage::new();
        self.proposals = ProposalCollector::new();
        self.votes = VoteCollector::new();
        //TODO: 将之前的 wal 文件备份
        let wal_path = &self.wal_log.dir;
        let _ = fs::remove_dir_all(wal_path);
        self.wal_log = Wal::new(wal_path).unwrap();
        self.consensus_power = false;
    }
}

#[inline]
pub fn get_total_weight(authorities: &[Node]) -> u64 {
    let weight: Vec<u64> = authorities
        .iter()
        .map(|node| u64::from(node.vote_weight))
        .collect();
    weight.iter().sum()
}

#[inline]
pub fn get_votes_weight(authorities: &[Node], vote_addresses: &[Address]) -> u64 {
    let votes_weight: Vec<u64> = authorities
        .iter()
        .filter(|node| vote_addresses.contains(&node.address))
        .map(|node| u64::from(node.vote_weight))
        .collect();
    votes_weight.iter().sum()
}

pub fn combine_two(first: &[u8], second: &[u8]) -> Vec<u8> {
    let first_len = first.len() as u64;
    let len_mark = first_len.to_be_bytes();
    let mut encode = Vec::with_capacity(8 + first.len() + second.len());
    encode.extend_from_slice(&len_mark);
    encode.extend_from_slice(first);
    encode.extend_from_slice(second);
    encode
}

pub fn extract_two(encode: &[u8]) -> BftResult<(&[u8], &[u8])> {
    let encode_len = encode.len();
    if encode_len < 8 {
        return Err(BftError::DecodeErr(format!(
            "extract_two failed, encode.len {} is less than 8",
            encode_len
        )));
    }
    let mut len: [u8; 8] = [0; 8];
    len.copy_from_slice(&encode[0..8]);
    let first_len = u64::from_be_bytes(len) as usize;
    if encode_len < first_len + 8 {
        return Err(BftError::DecodeErr(format!(
            "extract_two failed, encode.len {} is less than first_len + 8",
            encode_len
        )));
    }
    let (combine, two) = encode.split_at(first_len + 8);
    let (_, one) = combine.split_at(8);
    Ok((one, two))
}

pub fn encode_block(height: u64, block: &[u8], block_hash: &[u8]) -> Vec<u8> {
    let height_mark = height.to_be_bytes();
    let mut encode = Vec::with_capacity(8 + block.len());
    encode.extend_from_slice(&height_mark);
    let combine = combine_two(block_hash, block);
    encode.extend_from_slice(&combine);
    encode
}

pub fn decode_block(encode: &[u8]) -> BftResult<(u64, &[u8], &[u8])> {
    let (h, combine) = encode.split_at(8);
    let (block_hash, block) = extract_two(combine)?;
    let mut height_mark: [u8; 8] = [0; 8];
    height_mark.copy_from_slice(h);
    let height = u64::from_be_bytes(height_mark);
    Ok((height, block, block_hash))
}
