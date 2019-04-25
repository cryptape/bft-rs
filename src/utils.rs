use crate::*;
use crate::{
    algorithm::{Bft, INIT_HEIGHT, INIT_ROUND},
    collectors::{ProposalCollector, RoundCollector, VoteCollector, CACHE_N},
    error::{BftError, BftResult},
    objects::*,
    random::get_index,
    wal::Wal,
};
#[cfg(feature = "verify_req")]
use crossbeam_utils::thread as cross_thread;
use std::collections::HashMap;
use std::fs;
use std::time::Instant;

impl<T> Bft<T>
where
    T: BftSupport + 'static,
{
    pub(crate) fn clear(&mut self) {
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

    pub(crate) fn load_wal_log(&mut self) {
        // TODO: should prevent saving wal
        info!("Bft starts to load wal log!");
        let vec_buf = self.wal_log.load();
        for (log_type, msg) in vec_buf {
            match log_type {
                LogType::Proposal => {
                    info!("Load proposal!");
                    let _rst = self.send_bft_msg(BftMsg::Proposal(msg));
                }
                LogType::Vote => {
                    info!("Load vote message!");
                    let _rst = self.send_bft_msg(BftMsg::Vote(msg));
                }
                LogType::Feed => {
                    info!("Load feed message!");
                    let _rst = self.send_bft_msg(BftMsg::Feed(rlp::decode(&msg).unwrap()));
                }
                LogType::Status => {
                    info!("Load status message!");
                    let _rst = self.send_bft_msg(BftMsg::Status(rlp::decode(&msg).unwrap()));
                }
                #[cfg(feature = "verify_req")]
                LogType::VerifyResp => {
                    info!("Load verify_resp message!");
                    let _rst = self.send_bft_msg(BftMsg::VerifyResp(rlp::decode(&msg).unwrap()));
                }
            }
        }
        info!("Bft successfully processes the whole wal log!");
    }

    pub(crate) fn build_signed_proposal(&self, proposal: &Proposal) -> BftResult<SignedProposal> {
        let encode = rlp::encode(proposal);
        let hash = self.function.crypt_hash(&encode);

        let signature = self.function.sign(&hash).map_err(|e| {
            BftError::SignFailed(format!("{:?} of {:?}", e, proposal))
        })?;

        Ok(SignedProposal {
            proposal: proposal.clone(),
            signature,
        })
    }

    pub(crate) fn build_signed_vote(&self, vote: &Vote) -> BftResult<SignedVote> {
        let encode = rlp::encode(vote);
        let hash = self.function.crypt_hash(&encode);

        let signature = self.function.sign(&hash).map_err(|e| {
            BftError::SignFailed(format!("{:?} of {:?}", e, vote))
        })?;

        Ok(SignedVote {
            vote: vote.clone(),
            signature,
        })
    }

    pub(crate) fn get_vote_weight(&self, height: u64, address: &Address) -> u64 {
        if height != self.height {
            return 1u64;
        }
        if let Some(node) = self
            .authority_manage
            .authorities
            .iter()
            .find(|node| node.address == *address)
        {
            return node.vote_weight as u64;
        }
        1u64
    }

    fn get_authorities(&self, height: u64) -> BftResult<&Vec<Node>>{
        let p = &self.authority_manage;
        let mut authorities = &p.authorities;
        if height == p.authority_h_old {
            trace!("Bft sets the authority manage with old authorities!");
            authorities = &p.authorities_old;
        }
        if authorities.len() == 0 {
            return Err(BftError::ShouldNotHappen("the authority_list is empty".to_string()));
        }
        Ok(authorities)
    }

    pub(crate) fn get_proposer(&self, height: u64, round: u64) -> BftResult<&Address> {
        let authorities = self.get_authorities(height)?;
        let nonce = height + round;
        let weight: Vec<u64> = authorities
            .iter()
            .map(|node| node.proposal_weight as u64)
            .collect();
        let proposer: &Address = &authorities
            .get(get_index(nonce, &weight))
            .expect("The select proposer index is not in authorities, it should not happen!")
            .address;
        Ok(proposer)
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

    pub(crate) fn flush_cache(&mut self) -> BftResult<()>{
        let proposals = &mut self.proposals.proposals;
        let round_proposals = proposals.get_mut(&self.height);
        let votes = &mut self.votes.votes;
        let round_votes = votes.get_mut(&self.height);
        let mut round_collector = RoundCollector::new();
        if round_votes.is_some() {
            round_collector = round_votes.unwrap().clone();
            self.votes.remove(self.height);
        }

        if let Some(round_proposals) = round_proposals {
            for (_, signed_proposal) in round_proposals.round_proposals.iter() {
                let encode = rlp::encode(signed_proposal);
                let msg = BftMsg::Proposal(encode);
                let info = format!("{:?}", &msg);
                self.msg_sender.send(msg)
                    .map_err(|_| BftError::SendMsgErr(info))?;
            }
        };

        for (_, step_votes) in round_collector.round_votes.iter() {
            for (_, vote_set) in step_votes.step_votes.iter() {
                for (_, signed_vote) in vote_set.votes_by_sender.iter() {
                    let encode = rlp::encode(signed_vote);
                    let msg = BftMsg::Proposal(encode);
                    let info = format!("{:?}", &msg);
                    self.msg_sender.send(msg)
                        .map_err(|_| BftError::SendMsgErr(info))?;
                }
            }
        }

        Ok(())
    }

    #[cfg(feature = "verify_req")]
    pub(crate) fn save_verify_res(&mut self, round: Round, is_pass: bool) -> BftResult<()>{
        if self.verify_results.contains_key(&round) {
            if is_pass != *self.verify_results.get(&round).unwrap() {
                return Err(BftError::ShouldNotHappen(format!("get conflict verify result of round: {}", round)));
            }
        }
        self.verify_results
            .entry(round)
            .or_insert(is_pass);
        Ok(())
    }

    pub(crate) fn check_and_save_proposal(
        &mut self,
        signed_proposal: &SignedProposal,
        encode: &Encode,
        need_wal: bool,
    ) -> BftResult<()> {
        let proposal = &signed_proposal.proposal;
        let height = proposal.height;
        let round = proposal.round;

        if height < self.height - 1 {
            return Err(BftError::ObsoleteMsg(format!("{:?}", signed_proposal)));
        }

        let address = self
            .function
            .check_sig(&signed_proposal.signature, &self.function.crypt_hash(&rlp::encode(proposal)))
            .map_err(|e| {
                BftError::CheckSigFailed(format!("{:?} of {:?}", e, signed_proposal))
            })?;
        if proposal.proposer != address {
            return Err(BftError::InvalidSender(format!("recovers {:?} of {:?}", address, signed_proposal)));
        }

        // TODO: filter higher proposals to prevent flooding
        // prevent too many higher proposals flush out current proposal
        // because self.height - 1 is allowed, so judge height < self.height + CACHE_N - 1
        if height < self.height + CACHE_N - 1 && round < self.round + CACHE_N {
            self.proposals.add(&proposal)?;
            if need_wal {
                self.wal_log
                    .save(height, LogType::Proposal, &rlp::encode(signed_proposal))
                    .or(Err(BftError::SaveWalErr(format!("{:?}", signed_proposal))))?
            }
        }

        if height > self.height || (height == self.height && round >= self.round + CACHE_N){
            return Err(BftError::HigherMsg(format!("{:?}", signed_proposal)));
        }

        self.check_proposer(proposal)?;
        self.check_lock_votes(proposal, &self.function.crypt_hash(&proposal.block))?;

        if height == self.height - 1 {
            return Ok(());
        }

        self.check_block_txs(proposal, &self.function.crypt_hash(encode))?;
        self.check_proof(height, &proposal.proof)?;

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

        let vote_hash = self.function.crypt_hash(&rlp::encode(vote));
        let address = self
            .function
            .check_sig(&signed_vote.signature, &vote_hash)
            .map_err(|e| {
                BftError::CheckSigFailed(format!("{:?} of {:?}", e, signed_vote))
            })?;
        if vote.voter != address {
            return Err(BftError::InvalidSender(format!("recovers {:?} of {:?}", address, signed_vote)));
        }

        // prevent too many high proposals flush out current proposal
        if height < self.height + CACHE_N && round < self.round + CACHE_N {
            let vote_weight = self.get_vote_weight(vote.height, &vote.voter);
            self.votes.add(&signed_vote, vote_weight, self.height);
            if need_wal {
                self.wal_log
                    .save(height, LogType::Vote, &rlp::encode(signed_vote))
                    .or(Err(BftError::SaveWalErr(format!("{:?}", signed_vote))))?;
            }
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
            self.wal_log
                .save(height + 1, LogType::Status, &rlp::encode(status))
                .or(Err(BftError::SaveWalErr(format!("{:?}", status))))?;
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
            self.wal_log
                .save(self.height, LogType::VerifyResp, &rlp::encode(verify_resp))
                .or(Err(BftError::SaveWalErr(format!("{:?}", verify_resp))))?;
        }
        self.save_verify_res(verify_resp.round, verify_resp.is_pass)?;

        Ok(())
    }

    pub(crate) fn check_and_save_feed(&mut self, feed: &Feed, need_wal: bool) -> BftResult<()> {
        let height = feed.height;
        if height < self.height {
            return Err(BftError::ObsoleteMsg(format!("{:?}", feed)));
        }

        if height > self.height {
            return Err(BftError::HigherMsg(format!("{:?}", feed)));
        }

        if need_wal {
            self.wal_log
                .save(height, LogType::Feed, &rlp::encode(feed))
                .or(Err(BftError::SaveWalErr(format!("{:?}", feed))))?;
        }

        self.feed = Some(feed.block.clone());
        Ok(())
    }

    pub(crate) fn check_block_txs(
        &mut self,
        proposal: &Proposal,
        proposal_hash: &[u8],
    ) -> BftResult<()> {
        let height = proposal.height;
        let round = proposal.round;
        let block = &proposal.block;

        #[cfg(not(feature = "verify_req"))]
        {
            self.function.check_block(block, height).map_err(|e| {
                BftError::CheckBlockFailed(format!("{:?} of {:?}", e, proposal))
            })?;
            self.function.check_txs(block, proposal_hash, height, round)
                .map_err(|e| {
                    BftError::CheckTxFailed(format!("{:?} of {:?}", e, proposal))
                })?;
            Ok(())
        }

        #[cfg(feature = "verify_req")]
        {
            if let Err(e) = self.function.check_block(block, height){
                self.save_verify_res(round, false)?;
                return Err(BftError::CheckBlockFailed(format!("{:?} of {:?}", e, proposal)));
            }

            let function = self.function.clone();
            let sender = self.msg_sender.clone();
            cross_thread::scope(|s| {
                s.spawn(move |_| {
                    let is_pass = match function.check_txs(block, proposal_hash, height, round) {
                        Ok(_) => true,
                        Err(e) => {
                            warn!("Bft encounters BftError::CheckTxsFailed({:?} of {:?})", e, proposal);
                            false
                        }
                    };
                    let verify_resp = VerifyResp {
                        is_pass,
                        round,
                    };
                    sender.send(BftMsg::VerifyResp(verify_resp)).expect("cross_thread send verify_resp failed!");
                });
            })
            .unwrap();
            Ok(())
        }
    }

    pub(crate) fn check_proof(&mut self, height: u64, proof: &Proof) -> BftResult<()> {
        if height != self.height {
            return Err(BftError::ShouldNotHappen(format!("check_proof for {:?}", proof)));
        }

        let authorities = self.get_authorities(height)?;
        self.check_proof_only(&proof, height, authorities)?;
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
            return Err(BftError::CheckProofFailed(format!("height {} can not check for {:?}", height, proof)));
        }

        let vote_addresses: Vec<Address> = proof
            .precommit_votes
            .iter()
            .map(|(voter, _)| voter.clone())
            .collect();

        if get_votes_weight(authorities, &vote_addresses) * 3 <= get_total_weight(authorities) * 2 {
            return Err(BftError::CheckProofFailed(format!("not reach 2/3+ weight for {:?}", proof)));
        }

        let authority_addresses: Vec<Address> = authorities.iter().map(|node| node.address.clone()).collect();
        let mut map = HashMap::new();
        for (voter, sig) in proof.precommit_votes.clone(){
            // check repeat
            if let Some(_) = map.insert(voter.clone(), 1) {
                return Err(BftError::CheckProofFailed(format!("vote repeat of {:?} in {:?}", &voter, proof)));
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
                    .map_err(|e| BftError::CheckProofFailed(format!("{:?}, sig {:?} in {:?}", e, sig, proof)))?;
                if address != voter {
                    return Err(BftError::CheckProofFailed(format!("recover {:?} by voter {:?} in {:?}", &address, &voter, proof)));
                }
            } else {
                return Err(BftError::CheckProofFailed(format!("voter {:?} invalid in {:?}", &voter, proof)));
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
            return Err(BftError::ShouldNotHappen(format!("check_lock_votes for {:?}", proposal)));
        }

        let mut map = HashMap::new();
        if let Some(lock_round) = proposal.lock_round {
            for signed_vote in &proposal.lock_votes {
                let voter = self.check_vote(height, lock_round, block_hash, signed_vote)?;
                if let Some(_) = map.insert(voter, 1) {
                    return Err(BftError::CheckLockVotesFailed(format!("vote repeat of {:?} in {:?} with lock_votes {:?}", signed_vote, proposal, &proposal.lock_votes)));
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
        Err(BftError::CheckLockVotesFailed(format!("less than 2/3+ weight of {:?} with lock_votes {:?}", proposal, &proposal.lock_votes)))
    }

    pub(crate) fn check_vote(
        &mut self,
        height: Height,
        round: Round,
        block_hash: &Hash,
        signed_vote: &SignedVote,
    ) -> BftResult<Address> {
        if height < self.height - 1 {
            return Err(BftError::ShouldNotHappen(format!("check_vote for {:?}", signed_vote)));
        }

        let vote = &signed_vote.vote;
        if vote.height != height || vote.round != round {
            return Err(BftError::CheckLockVotesFailed(format!("vote {:?} mismatching height: {} or round: {}", signed_vote, height, round)));
        }

        if vote.block_hash != block_hash.to_vec() {
            warn!(
                "The lock votes of proposal {:?} contains vote for other proposal hash {:?}!",
                block_hash, vote.block_hash
            );
            return Err(BftError::CheckLockVotesFailed(format!("vote {:?} not for rightful block_hash {:?}", vote, block_hash)));
        }

        let authorities = self.get_authorities(height)?;
        let voter = &vote.voter;
        if !authorities.iter().any(|node| &node.address == voter) {
            return Err(BftError::CheckLockVotesFailed(format!("the voter {:?} not in authorities", voter)));
        }

        let signature = &signed_vote.signature;
        let vote_hash = self.function.crypt_hash(&rlp::encode(vote));
        let address = self
            .function
            .check_sig(signature, &vote_hash)
            .map_err(|e| {
                BftError::CheckLockVotesFailed(format!("check sig failed with {:?} of {:?}", e, signed_vote))
            })?;
        if &address != voter {
            warn!("The address recovers from the signature is {:?} which is mismatching with the sender {:?}!", &address, &voter);
            return Err(BftError::CheckLockVotesFailed(format!("recover {:?} of {:?}", &address, signed_vote)));
        }

        let vote_weight = self.get_vote_weight(height, &voter);
        self.votes.add(&signed_vote, vote_weight, self.height);
        Ok(address)
    }

    pub(crate) fn check_proposer(
        &self,
        proposal: &Proposal,
    ) -> BftResult<()> {
        let height = proposal.height;
        let round = proposal.round;
        let address = &proposal.proposer;

        if height < self.height - 1 || height > self.height {
            return Err(BftError::ShouldNotHappen(format!("check_proposer for {:?}", proposal)));
        }
        let proposer = self.get_proposer(height, round)?;
        if proposer == address {
            Ok(())
        } else {
            Err(BftError::InvalidSender(format!("the rightful proposer is {:?} by {:?}", proposer, proposal)))
        }
    }

    pub(crate) fn check_voter(&self, vote: &Vote) -> BftResult<()> {
        let height = vote.height;
        let voter = &vote.voter;

        if height < self.height - 1 || height > self.height {
            return Err(BftError::ShouldNotHappen(format!("check_voter for {:?}", vote)));
        }

        let authorities = self.get_authorities(height)?;

        if !authorities.iter().any(|node| node.address == *voter) {
            return Err(BftError::InvalidSender(format!("the {:?} of {:?} not in authorities", voter, vote)));
        }

        Ok(())
    }

    #[inline]
    pub(crate) fn send_bft_msg(&self, msg: BftMsg) -> BftResult<()>{
        let info = format!("{:?}", &msg);
        self.msg_sender.send(msg).map_err(|_| BftError::SendMsgErr(info))
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
}

fn get_total_weight(authorities: &[Node]) -> u64 {
    let weight: Vec<u64> = authorities
        .iter()
        .map(|node| node.vote_weight as u64)
        .collect();
    weight.iter().sum()
}

fn get_votes_weight(authorities: &[Node], vote_addresses: &[Address]) -> u64 {
    let votes_weight: Vec<u64> = authorities
        .iter()
        .filter(|node| vote_addresses.contains(&node.address))
        .map(|node| node.vote_weight as u64)
        .collect();
    votes_weight.iter().sum()
}
