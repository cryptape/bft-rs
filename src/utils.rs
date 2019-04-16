use crate::*;
use crate::{
    algorithm::{Bft, INIT_HEIGHT, INIT_ROUND},
    collectors::{ProposalCollector, VoteCollector, RoundCollector},
    error::BftError,
    objects::*,
    random::get_proposer,
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

    pub(crate) fn load_wal_log(&mut self) {
        // TODO: should prevent saving wal
        info!("Bft starts to load wal log!");
        let vec_buf = self.wal_log.load();
        for (log_type, msg) in vec_buf {
            match log_type {
                LogType::Proposal => {
                    info!("Load proposal!");
                    self.send_bft_msg(BftMsg::Proposal(msg));
                }
                LogType::Vote => {
                    info!("Load vote message!");
                    self.send_bft_msg(BftMsg::Vote(msg));
                }
                LogType::Feed => {
                    info!("Load feed message!");
                    self.send_bft_msg(BftMsg::Feed(rlp::decode(&msg).unwrap()));
                }
                LogType::Status => {
                    info!("Load status message!");
                    self.send_bft_msg(BftMsg::Status(rlp::decode(&msg).unwrap()));
                }
                #[cfg(feature = "verify_req")]
                LogType::VerifyResp => {
                    info!("Load verify_resp message!");
                    self.send_bft_msg(BftMsg::VerifyResp(rlp::decode(&msg).unwrap()));
                }
            }
        }
        info!("Bft successfully processes the whole wal log!");
    }

    pub(crate) fn build_signed_proposal(&self, proposal: &Proposal) -> SignedProposal{
        let encode = rlp::encode(proposal);
        let hash = self.function.crypt_hash(&encode);
        let signature = self.function.sign(&hash).unwrap();

        SignedProposal{
            proposal: proposal.clone(),
            signature,
        }
    }

    pub(crate) fn build_signed_vote(&self, vote: &Vote) -> SignedVote{
        let encode =  rlp::encode(vote);
        let hash = self.function.crypt_hash(&encode);
        let signature = self.function.sign(&hash).unwrap();
        SignedVote {
            vote: vote.clone(),
            signature,
        }
    }

    pub(crate) fn get_vote_weight(&self, height: u64, address: &Address) -> u64 {
        if height != self.height{
            return 1u64;
        }
        if let Some(node) = self.authority_manage.authorities.iter()
            .find(|node| node.address == *address){
            return node.vote_weight as u64;
        }
        1u64
    }

    pub(crate) fn generate_proof(&mut self, lock_status: LockStatus) -> Proof {
        info!("Bft generate proof with lock_status {:?}", lock_status);
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

    pub(crate) fn flush_cache(&mut self) {
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

    pub(crate) fn save_verify_res(&mut self, block_hash: &[u8], is_pass: bool) {
        if self.verify_results.contains_key(block_hash) {
            if is_pass != *self.verify_results.get(block_hash).unwrap() {
                error!("The verify results of {:?} are conflict!", block_hash);
                return;
            }
        }
        self.verify_results.entry(block_hash.to_vec()).or_insert(is_pass);
    }

    pub(crate) fn check_and_save_proposal(&mut self, signed_proposal: &SignedProposal, need_wal: bool) -> BftResult<()> {
        let proposal = &signed_proposal.proposal;
        let height = proposal.height;
        let round = proposal.round;

        if height < self.height - 1 {
            info!("The height of proposal is {} which is obsolete compared to self.height {}!", height, self.height);
            return Err(BftError::ObsoleteMsg);
        }

        let proposal_encode = rlp::encode(proposal);
        let proposal_hash = self.function.crypt_hash(&proposal_encode);
        info!("Bft calculates proposal_hash {:?}", proposal_hash);
        let address = self.function.check_sig(&signed_proposal.signature, &proposal_hash).ok_or(BftError::CheckSigFailed)?;
        if proposal.proposer != address {
            warn!("Bft expects proposer's address {:?} while get {:?} from check_sig", proposal.proposer, address);
            return Err(BftError::MismatchingProposer);
        }

        let block = &proposal.block;

        // check_prehash should involved in check_block
        self.check_block(block, height)?;
        info!("Bft check_block successed!");
        if need_wal {
            let encode: Vec<u8> = rlp::encode(signed_proposal);
            self.wal_log.save(height, LogType::Proposal, &encode).or(Err(BftError::SaveWalErr))?
        }

        self.proposals.add(&proposal);

        if height > self.height {
            return Err(BftError::HigherMsg);
        }

        self.check_proposer(height, round, &address)?;
        info!("Bft check_proposer successed!");

        let block_hash = self.function.crypt_hash(block);
        self.check_lock_votes(proposal, &block_hash)?;
        info!("Bft check_lock_votes successed!");

        if height < self.height {
            return Ok(());
        }

        self.check_proof(height, &proposal.proof)?;
        info!("Bft check_proof successed!");

        Ok(())
    }

    pub(crate) fn check_and_save_vote(&mut self, signed_vote: &SignedVote, need_wal: bool) -> BftResult<()> {
        let vote = &signed_vote.vote;
        let height = vote.height;
        if height < self.height - 1 {
            warn!("The height of raw_bytes is {} which is obsolete compared to self.height {}!", height, self.height);
            return Err(BftError::ObsoleteMsg);
        }

        let vote_encode = rlp::encode(vote);
        let vote_hash = self.function.crypt_hash(&vote_encode);
        let address = self.function.check_sig(&signed_vote.signature, &vote_hash).ok_or(BftError::CheckSigFailed)?;
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

    pub(crate) fn check_and_save_status(&mut self, status: &Status, need_wal: bool) -> BftResult<()> {
        let height = status.height;
        if self.height > 0 && height < self.height - 1 {
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
    pub(crate) fn check_and_save_verify_resp(&mut self, verify_resp: &VerifyResp, need_wal: bool) -> BftResult<()> {
        if need_wal {
            let msg: Vec<u8> = rlp::encode(verify_resp);
            self.wal_log.save(self.height, LogType::VerifyResp, &msg).or( Err(BftError::SaveWalErr))?;
        }
        self.save_verify_res(&verify_resp.block_hash, verify_resp.is_pass);

        Ok(())
    }

    pub(crate) fn check_and_save_feed(&mut self, feed: Feed, need_wal: bool) -> BftResult<()> {
        let height = feed.height;
        if height < self.height {
            warn!("the height of block_txs is {}, while self.height is {}", height, self.height);
            return Err(BftError::ObsoleteMsg);
        }
        if need_wal {
            let msg: Vec<u8> = rlp::encode(&feed);
            self.wal_log.save(height, LogType::Feed, &msg).or(Err(BftError::SaveWalErr))?;
        }

        self.feeds.insert(height, feed.block);

        if height > self.height{
            return Err(BftError::HigherMsg);
        }
        Ok(())
    }

    pub(crate) fn check_block(&mut self, block: &[u8], height: u64) -> BftResult<()> {
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
                if !self.function.check_block(block, height) {
                    self.save_verify_res(&block_hash, false);
                    return Err(BftError::CheckBlockFailed);
                }

                let mut function = self.function.clone();
                let sender = self.msg_sender.clone();
                let height = self.height;
                let round = self.round;
                cross_thread::scope(|s|{
                    s.spawn(move |_|{
                        let is_pass = function.check_transaction(block, height, round);
                        let block_hash = function.crypt_hash(block);
                        let verify_resp = VerifyResp{is_pass, block_hash};
                        info!("Bft send verifyResp {:?} in check_block", verify_resp);
                        sender.send(BftMsg::VerifyResp(verify_resp)).unwrap();
                    });
                }).unwrap();
                Ok(())
            }
    }

    pub(crate) fn check_proof(&mut self, height: u64, proof: &Proof) -> BftResult<()> {
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
            info!("Bft sets self.proof from received signed_proposal!");
            self.proof = Some(proof.clone());
        }

        Ok(())
    }

    pub(crate) fn check_proof_only(&self, proof: &Proof, height: u64, authorities: &[Node]) -> bool {
        if proof.height == 0 {
            return true;
        }
        if height != proof.height + 1 {
            error!("Bft check proof failed, the proof.height is {}", proof.height);
            return false;
        }

        let weight: Vec<u64> = authorities.iter().map(|node| node.vote_weight as u64).collect();
        let vote_addresses : Vec<Address> = proof.precommit_votes.iter().map(|(voter, _)| voter.clone()).collect();
        let votes_weight: Vec<u64> = authorities.iter()
            .filter(|node| vote_addresses.contains(&node.address))
            .map(|node| node.vote_weight as u64).collect();
        let weight_sum: u64 = weight.iter().sum();
        let vote_sum: u64 = votes_weight.iter().sum();
        if vote_sum * 3 <= weight_sum * 2 {
            error!("Bft check proof failed, the votes:weight is {}:{}", vote_sum, weight_sum);
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
                if let Some(address) = self.function.check_sig(sig, &self.function.crypt_hash(&msg)) {
                    info!("Bft check_proof_only get address {:?} while voter is {:?}", address, voter);
                    return address == *voter;
                }
            }
            false
        })
    }

    pub(crate) fn check_lock_votes(&mut self, proposal: &Proposal, block_hash: &[u8]) -> BftResult<()> {
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

    pub(crate) fn check_vote(&mut self, height: u64, round: u64, block_hash: &[u8], signed_vote: &SignedVote) -> BftResult<Address> {
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
            info!("Bft sets the authority manage with old authorities!");
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
        let address = self.function.check_sig(signature, &vote_hash).ok_or(BftError::CheckSigFailed)?;
        if &address != voter {
            error!("The address recovers from the signature is {:?} which is mismatching with the sender {:?}!", &address, &voter);
            return Err(BftError::MismatchingVoter);
        }

        let vote_weight = self.get_vote_weight(height, &voter);
        self.votes.add(&signed_vote, vote_weight, self.height);
        Ok(address)
    }

    pub(crate) fn check_proposer(&self, height: u64, round: u64, address: &Address) -> BftResult<()> {
        if height < self.height - 1 || height > self.height{
            error!("The proposer's height is {} compared to self.height {}, which should not happen!", height, self.height);
            return Err(BftError::ShouldNotHappen);
        }
        let p = &self.authority_manage;
        let mut authorities = &p.authorities;
        if height == p.authority_h_old {
            info!("Bft sets the authority manage with old authorities!");
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

    pub(crate) fn check_voter(&self, height: u64, address: &Address) -> BftResult<()> {
        if height < self.height - 1 || height > self.height{
            error!("The height is {} compared to self.height {}, which should not happen!", height, self.height);
            return Err(BftError::ShouldNotHappen);
        }
        let p = &self.authority_manage;
        let mut authorities = &p.authorities;
        if height == p.authority_h_old {
            info!("Bft sets the authority manage with old authorities!");
            authorities = &p.authorities_old;
        }

        if !authorities.iter().any(|node| node.address == *address) {
            error!("The lock votes contains vote with invalid voter {:?}!", address);
            return Err(BftError::InvalidVoter);
        }

        Ok(())
    }

    #[inline]
    pub(crate) fn send_bft_msg(&self, msg: BftMsg) {
        info!("Bft sends bft msg!");
        self.msg_sender.send(msg).unwrap();
    }

}
