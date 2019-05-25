use crate::*;
use crate::{
    algorithm::{Bft, TIMEOUT_RETRANSE_COEF},
    objects::*,
};
use rand::prelude::*;

impl<T> Bft<T>
where
    T: BftSupport + 'static,
{
    pub(crate) fn transmit_byzantine_proposal(&mut self) -> BftResult<()> {
        self.send_byzantine_proposal()?;
        self.send_byzantine_proposal()?;
        self.send_byzantine_proposal()?;

        if self.step == Step::ProposeWait {
            self.transmit_prevote(false)?;
        }
        Ok(())
    }

    pub(crate) fn transmit_byzantine_prevote(&mut self, resend: bool) -> BftResult<()> {
        self.send_byzantine_vote(VoteType::Prevote)?;
        self.send_byzantine_vote(VoteType::Prevote)?;
        self.send_byzantine_vote(VoteType::Prevote)?;

        if !resend {
            self.change_to_step(Step::Prevote);
        }
        self.set_timer(
            self.params.timer.get_prevote() * TIMEOUT_RETRANSE_COEF,
            Step::Prevote,
        );
        Ok(())
    }

    pub(crate) fn transmit_byzantine_precommit(&mut self, resend: bool) -> BftResult<()> {
        self.send_byzantine_vote(VoteType::Precommit)?;
        self.send_byzantine_vote(VoteType::Precommit)?;
        self.send_byzantine_vote(VoteType::Precommit)?;

        if !resend {
            self.change_to_step(Step::Precommit);
        }
        self.set_timer(
            self.params.timer.get_precommit() * TIMEOUT_RETRANSE_COEF,
            Step::Prevote,
        );
        Ok(())
    }

    pub(crate) fn retransmit_byzantine_lower_votes(&self) -> BftResult<()> {
        Ok(())
    }

    pub(crate) fn retransmit_byzantine_nil_precommit(&self) -> BftResult<()> {
        Ok(())
    }

    fn send_byzantine_proposal(&mut self) -> BftResult<()> {
        let block = get_rand_vec(20);
        let block_hash = self.function.crypt_hash(&block);
        self.blocks.add(self.height, &block_hash, &block.into());
        self.block_hash = Some(block_hash.clone());

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
        self.function.transmit(BftMsg::Proposal(encode).clone());
        Ok(())
    }

    fn send_byzantine_vote(&mut self, vote_type: VoteType) -> BftResult<()> {
        let vote = Vote {
            vote_type,
            height: self.height,
            round: self.round,
            block_hash: self.get_rand_hash(),
            voter: self.params.address.clone(),
        };

        let encode = rlp::encode(&vote);
        let hash = self.function.crypt_hash(&encode);
        let signature = self
            .function
            .sign(&hash)
            .map_err(|e| BftError::SignFailed(format!("{:?} of {:?}", e, vote)))?;
        let signed_vote = SignedVote {
            vote: vote.clone(),
            signature,
        };
        self.function
            .transmit(BftMsg::Vote(rlp::encode(&signed_vote).into()));
        Ok(())
    }

    fn get_rand_hash(&self) -> Hash {
        self.function.crypt_hash(&get_rand_vec(20))
    }
}

fn get_rand_vec(len: usize) -> Vec<u8> {
    let mut vec = Vec::with_capacity(len);
    let mut rng = rand::thread_rng();
    for _ in 0..len {
        vec.push(rng.gen::<u8>());
    }
    vec
}
