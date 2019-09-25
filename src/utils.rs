use crate::*;
use crate::{
    algorithm::{Bft, INIT_HEIGHT, INIT_ROUND},
    collectors::{ProposalCollector, RoundCollector, VoteCollector, VoteSet, CACHE_N},
    error::{handle_err, BftError, BftResult},
    objects::*,
    timer::TimeoutInfo,
    wal::Wal,
};
use bincode::{serialize, Infinite};
use cita_types::{Address as CitaAddr, H256};
#[cfg(feature = "verify_req")]
#[allow(unused_imports)]
use log::{log, warn};
use proof::Step as CitaStep;
#[cfg(feature = "random_proposer")]
use rand_core::{RngCore, SeedableRng};
#[cfg(feature = "random_proposer")]
use rand_pcg::Pcg64Mcg as Pcg;
#[cfg(feature = "verify_req")]
use std::collections::HashMap;
use std::fs;
#[cfg(feature = "verify_req")]
use std::thread;
use std::time::{Duration, Instant};

const TIMEOUT_LOW_HEIGHT_MESSAGE_COEF: u32 = 20;
const TIMEOUT_LOW_ROUND_MESSAGE_COEF: u32 = 20;

impl<T> Bft<T>
where
    T: BftSupport + 'static,
{
    pub(crate) fn load_wal_log(&mut self) {
        info!("Node {:?} starts loading wal log!", self.params.address);
        let vec_buf = self.wal_log.load();
        for (log_type, encode) in vec_buf {
            handle_err(self.process_wal_log(log_type, encode), &self.params.address);
        }
        info!(
            "Node {:?} successfully processed the whole wal log!",
            self.params.address
        );
    }

    fn process_wal_log(&mut self, log_type: LogType, encode: Vec<u8>) -> BftResult<()> {
        match log_type {
            LogType::Proposal => {
                info!("Node {:?} loads proposal", self.params.address);
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
                let proposal_block_encode = combine_two(&encode, &block);
                self.process(BftMsg::Proposal(proposal_block_encode), false)?;
            }
            LogType::Vote => {
                info!("Node {:?} loads vote", self.params.address);
                self.process(BftMsg::Vote(encode), false)?;
            }
            LogType::Feed => {
                info!("Node {:?} loads feed", self.params.address);
                let feed: Feed = rlp::decode(&encode)
                    .map_err(|e| BftError::DecodeErr(format!("feed encounters {:?}", e)))?;
                self.process(BftMsg::Feed(feed), false)?;
            }
            LogType::Status => {
                info!("Node {:?} loads status", self.params.address);
                let status: Status = rlp::decode(&encode)
                    .map_err(|e| BftError::DecodeErr(format!("status encounters {:?}", e)))?;
                self.process(BftMsg::Status(status), false)?;
            }
            LogType::Proof => {
                info!("Node {:?} loads proof", self.params.address);
                let proof: Proof = rlp::decode(&encode)
                    .map_err(|e| BftError::DecodeErr(format!("proof encounters {:?}", e)))?;
                self.set_proof(&proof, false);
            }
            LogType::VerifyResp => {
                info!("Node {:?} loads verify_resp", self.params.address);
                let verify_resp: VerifyResp = rlp::decode(&encode)
                    .map_err(|e| BftError::DecodeErr(format!("verify_resp encounters {:?}", e)))?;
                self.process(BftMsg::VerifyResp(verify_resp), false)?;
            }

            LogType::TimeOutInfo => {
                info!("Node {:?} loads timeout_info", self.params.address);
                let time_out_info: TimeoutInfo = rlp::decode(&encode).map_err(|e| {
                    BftError::DecodeErr(format!("time_out_info encounters {:?}", e))
                })?;
                self.timeout_process(time_out_info, false)?;
            }

            LogType::Block => {
                info!("Node {:?} loads block", self.params.address);
                let (height, block, block_hash) = decode_block(&encode)?;
                self.blocks.add(height, &block_hash, &block);
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
                BftError::ShouldNotHappen(format!(
                    "can not fetch block {:?} from cache when send signed_proposal",
                    proposal.height
                ))
            })?;
        let encode = combine_two(&signed_proposal_encode, &block);
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
        //        let encode = rlp::encode(vote);
        // compatibility with CITA
        let encode = encode_compatible_with_cita(vote);
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
    fn get_authorities(&self, height: Height) -> BftResult<&Vec<Node>> {
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
    fn get_vote_weight(&self, height: Height, address: &Address) -> u64 {
        if height != self.height {
            return 1;
        }
        if let Some(node) = self
            .authority_manage
            .authorities
            .iter()
            .find(|node| &node.address == address)
        {
            return u64::from(node.vote_weight);
        }
        1
    }

    pub(crate) fn get_proposer(&self, height: Height, round: Round) -> BftResult<&Address> {
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
    pub(crate) fn set_proof(&mut self, proof: &Proof, need_wal: bool) {
        if self.proof.height < proof.height {
            self.proof = proof.clone();
            if need_wal {
                self.save_proof(proof.height, proof);
            }
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

    pub(crate) fn set_polc(&mut self, hash: &Hash, voteset: &VoteSet) {
        self.block_hash = Some(hash.to_owned());
        self.lock_status = Some(LockStatus {
            block_hash: hash.to_owned(),
            round: self.round,
            votes: voteset.extract_polc(hash),
        });

        debug!(
            "Node {:?} sets a PoLC on block_hash {:?} at h:{:?} r:{:?} ",
            self.params.address,
            hash.to_owned(),
            self.height,
            self.round
        );
    }

    #[inline]
    pub(crate) fn set_timer(&self, duration: Duration, step: Step) {
        debug!(
            "Node {:?} will process {:?} after {:?}",
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
        self.fetch_proposal(self.height, 0)?;
        self.fetch_votes(self.height)?;
        Ok(())
    }

    pub(crate) fn fetch_proposal(&mut self, height: Height, round: Round) -> BftResult<()> {
        let opt = self.proposals.get_proposal(height, round).clone();
        if let Some(signed_proposal) = opt {
            self.proposals.remove(height, round);

            let block_hash = signed_proposal.proposal.block_hash.clone();
            let block = self.blocks.get_block(height, &block_hash).ok_or_else(|| {
                BftError::ShouldNotHappen(
                    "can not fetch block from cache when load signed_proposal".to_string(),
                )
            })?;
            let proposal_encode = rlp::encode(&signed_proposal);
            let encode = combine_two(&proposal_encode, &block);
            let msg = BftMsg::Proposal(encode);
            let info = format!("{:?}", &msg);
            self.msg_sender
                .send(msg)
                .map_err(|_| BftError::SendMsgErr(info))?;
        }
        Ok(())
    }

    pub(crate) fn fetch_votes(&mut self, height: Height) -> BftResult<()> {
        let votes = &mut self.votes.votes;
        let round_votes = votes.get_mut(&height);
        let mut vote_collector = RoundCollector::new();

        if round_votes.is_some() {
            vote_collector = round_votes.unwrap().clone();
            self.votes.remove(height);
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

    pub(crate) fn save_verify_res(
        &mut self,
        round: Round,
        verify_resp: &VerifyResp,
    ) -> BftResult<()> {
        if self.verify_results.contains_key(&round)
            && verify_resp.is_pass != self.verify_results[&round].is_pass
        {
            Err(BftError::ShouldNotHappen(format!(
                "get conflict verify result of round: {}",
                round
            )))
        } else {
            self.verify_results
                .entry(round)
                .or_insert_with(|| verify_resp.clone());
            Ok(())
        }
    }

    fn save_proof(&mut self, height: Height, proof: &Proof) {
        debug!("save {:?}", proof);
        handle_err(
            self.wal_log
                .save(height, LogType::Proof, &rlp::encode(proof))
                .or_else(|e| {
                    Err(BftError::SaveWalErr(format!(
                        "{:?} of {:?}",
                        e, &self.proof
                    )))
                }),
            &self.params.address,
        );
    }

    pub(crate) fn check_and_save_proposal(
        &mut self,
        signed_proposal: &SignedProposal,
        block: &Block,
        signed_proposal_hash: &[u8],
        need_wal: bool,
    ) -> BftResult<()> {
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

        if height == self.height || height == self.height - 1 {
            self.check_proposer(proposal)?;
            self.check_lock_votes(proposal, block_hash)?;

            if height == self.height - 1 {
                return Ok(());
            }
            self.check_block_txs(
                proposal,
                block,
                &self.function.crypt_hash(signed_proposal_hash),
            )?;
            self.check_proof(height, &proposal.proof)?;
        }

        // prevent too many higher proposals flush out current proposal
        if height >= self.height && height < self.height + CACHE_N && round < self.round + CACHE_N {
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
                        &self.params.address,
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
                    &self.params.address,
                );
            }
        }

        if height > self.height || (height == self.height && round >= self.round + CACHE_N) {
            return Err(BftError::HigherMsg(format!("{:?}", signed_proposal)));
        }

        Ok(())
    }

    pub(crate) fn check_and_save_vote(
        &mut self,
        signed_vote: &SignedVote,
        need_wal: bool,
    ) -> BftResult<()> {
        let vote = &signed_vote.vote;
        let height = vote.height;
        let round = vote.round;
        if height < self.height - 1 {
            return Err(BftError::ObsoleteMsg(format!("{:?}", signed_vote)));
        }

        //        let vote_hash = self.function.crypt_hash(&rlp::encode(vote));
        // compatibility with cita
        let vote_hash = self.function.crypt_hash(&encode_compatible_with_cita(vote));

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

        if height == self.height {
            self.check_voter(vote)?;
        }

        // prevent too many high proposals flush out current proposal
        if height >= self.height && height < self.height + CACHE_N && round < self.round + CACHE_N {
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
                    &self.params.address,
                );
            }
            handle_err(result, &self.params.address);
        }

        if height > self.height || round >= self.round + CACHE_N {
            return Err(BftError::HigherMsg(format!("{:?}", signed_vote)));
        }

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
            self.save_proof(self.height + 1, &self.proof.clone());
            let status_height = status.height;
            handle_err(
                self.wal_log
                    .save(status_height + 1, LogType::Status, &rlp::encode(status))
                    .or_else(|e| Err(BftError::SaveWalErr(format!("{:?} of {:?}", e, status)))),
                &self.params.address,
            );
        }

        Ok(())
    }

    pub(crate) fn check_and_save_verify_resp(
        &mut self,
        verify_resp: &VerifyResp,
        need_wal: bool,
    ) -> BftResult<()> {
        if need_wal {
            handle_err(
                self.wal_log
                    .save(self.height, LogType::VerifyResp, &rlp::encode(verify_resp))
                    .or_else(|_| Err(BftError::SaveWalErr(format!("{:?}", verify_resp)))),
                &self.params.address,
            );
        }
        self.save_verify_res(verify_resp.round, verify_resp)?;

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
                &self.params.address,
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
        block: &Block,
        signed_proposal_hash: &Hash,
    ) -> BftResult<()> {
        let height = proposal.height;
        let round = proposal.round;
        let block_hash = &proposal.block_hash;

        #[cfg(not(feature = "verify_req"))]
        {
            let verify_resp = self
                .function
                .check_block(
                    block,
                    block_hash,
                    signed_proposal_hash,
                    (height, round),
                    proposal.lock_round.is_some(),
                    &proposal.proposer,
                )
                .map_err(|e| BftError::CheckBlockFailed(format!("{:?} of {:?}", e, proposal)))?;
            self.check_and_save_verify_resp(&verify_resp, false)?;
            if verify_resp.is_pass {
                Ok(())
            } else {
                Err(BftError::CheckBlockFailed(format!("of {:?}", proposal)))
            }
        }

        #[cfg(feature = "verify_req")]
        {
            let function = self.function.clone();
            let sender = self.msg_sender.clone();
            let block = block.clone();
            let block_hash = block_hash.clone();
            let is_lock = proposal.lock_round.is_some();
            let signed_proposal_hash = signed_proposal_hash.clone();
            let address = self.params.address.clone();
            let proposer = proposal.proposer.clone();
            thread::spawn(move || {
                match function.check_block(
                    &block,
                    &block_hash,
                    &signed_proposal_hash,
                    (height, round),
                    is_lock,
                    &proposer,
                ) {
                    Ok(verify_resp) => {
                        handle_err(
                            sender
                                .send(BftMsg::VerifyResp(verify_resp))
                                .map_err(|e| BftError::SendMsgErr(format!("{:?}", e))),
                            &address,
                        );
                    }
                    Err(e) => {
                        warn!(
                            "Node {:?} encounters BftError::CheckTxsFailed({:?})",
                            address, e
                        );
                    }
                };
            });

            Ok(())
        }
    }

    pub(crate) fn check_proof(&mut self, height: Height, proof: &Proof) -> BftResult<()> {
        if height != self.height {
            return Err(BftError::ShouldNotHappen(format!(
                "check_proof for {:?}",
                proof
            )));
        }

        let authorities = self.get_authorities(height)?;
        self.check_proof_only(proof, height, authorities)?;
        self.set_proof(proof, true);

        Ok(())
    }

    pub(crate) fn check_proof_only(
        &self,
        proof: &Proof,
        height: Height,
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
        for (voter, sig) in proof.precommit_votes.clone() {
            if authority_addresses.contains(&voter) {
                let vote = Vote {
                    vote_type: VoteType::Precommit,
                    height: proof.height,
                    round: proof.round,
                    block_hash: proof.block_hash.clone(),
                    voter: voter.clone(),
                };
                //                let msg = rlp::encode(&vote);
                // compatibility with cita
                let msg = encode_compatible_with_cita(&vote);
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
        block_hash: &Hash,
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
        block_hash: &Hash,
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

        if &vote.block_hash != block_hash {
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
        //        let vote_hash = self.function.crypt_hash(&rlp::encode(vote));
        // compatibility with cita
        let vote_hash = self.function.crypt_hash(&encode_compatible_with_cita(vote));
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

    pub(crate) fn filter_height(&self, voter: &Address) -> bool {
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

    pub(crate) fn filter_round(&self, voter: &Address) -> bool {
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
        info!(
            "Node {:?}, change step from {:?} to {:?}",
            self.params.address, self.step, step
        );
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
            "Node {:?} cleans PoLC at h:{}, r:{}",
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
        trace!("Node {:?} clean feed", self.params.address);
        self.feed = None;
    }

    #[inline]
    pub(crate) fn clean_filter(&mut self) {
        trace!("Node {:?} clean filter", self.params.address);
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

pub fn encode_block(height: Height, block: &Block, block_hash: &Hash) -> Vec<u8> {
    let height_mark = height.to_be_bytes();
    let mut encode = Vec::with_capacity(8 + block.0.len());
    encode.extend_from_slice(&height_mark);
    let combine = combine_two(&block_hash.0, &block);
    encode.extend_from_slice(&combine);
    encode
}

pub fn decode_block(encode: &[u8]) -> BftResult<(Height, Block, Hash)> {
    let (h, combine) = encode.split_at(8);
    let (block_hash, block) = extract_two(combine)?;
    let mut height_mark: [u8; 8] = [0; 8];
    height_mark.copy_from_slice(h);
    let height = Height::from_be_bytes(height_mark);
    Ok((height, block.into(), block_hash.into()))
}

#[cfg(feature = "random_proposer")]
pub(crate) fn get_index(seed: u64, weight: &[u64]) -> usize {
    let sum: u64 = weight.iter().sum();
    let x = u64::max_value() / sum;

    let mut rng = Pcg::seed_from_u64(seed);
    let mut res = rng.next_u64();
    while res >= sum * x {
        res = rng.next_u64();
    }
    let mut acc = 0;
    for (index, w) in weight.iter().enumerate() {
        acc += *w;
        if res < acc * x {
            return index;
        }
    }
    0
}

#[cfg(not(feature = "random_proposer"))]
pub(crate) fn get_index(seed: u64, weight: &[u64]) -> usize {
    let sum: u64 = weight.iter().sum();
    let x = seed % sum;

    let mut acc = 0;
    for (index, w) in weight.iter().enumerate() {
        acc += *w;
        if x < acc {
            return index;
        }
    }
    0
}

fn encode_compatible_with_cita(vote: &Vote) -> Vec<u8> {
    let h = vote.height as usize;
    let r = vote.round as usize;
    let step = if vote.vote_type == VoteType::Prevote {
        CitaStep::Prevote
    } else {
        CitaStep::Precommit
    };
    let sender = CitaAddr::from(vote.voter.0.as_slice());
    let proposal = H256::from(vote.block_hash.0.as_slice());
    serialize(&(h, r, step, sender, Some(proposal)), Infinite).unwrap()
}
