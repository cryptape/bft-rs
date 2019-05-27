extern crate bft_rs;

use self::bft_rs::*;
use super::config::Config;
use super::utils::*;
use crossbeam::crossbeam_channel::Sender;
use std::thread;

pub struct Support {
    pub config: Config,
    pub address: Address,
    pub msg_send: Sender<(BftMsg, Address)>,
    pub commit_send: Sender<(Commit, Address)>,
}

impl BftSupport for Support {
    type Error = TestError;
    fn check_block(
        &self,
        block: &Block,
        _block_hash: &Hash,
        _height: Height,
    ) -> Result<(), TestError> {
        if check_block_result(block) {
            Ok(())
        } else {
            Err(TestError::CheckBlockFailed)
        }
    }

    fn check_txs(
        &self,
        _block: &Block,
        _block_hash: &Hash,
        _signed_proposal_hash: &Hash,
        _height: Height,
        _round: Round,
    ) -> Result<(), TestError> {
        let delay = check_txs_delay(&self.config);
        thread::sleep(delay);
        if check_txs_result(&self.config) {
            Ok(())
        } else {
            Err(TestError::CheckTxsFailed)
        }
    }

    fn transmit(&self, msg: BftMsg) {
        self.msg_send.send((msg, self.address.clone())).unwrap();
    }

    fn commit(&self, commit: Commit) -> Result<Status, TestError> {
        let address = self.address.clone();
        self.commit_send.send((commit, address)).unwrap();
        Err(TestError::CommitProposed)
    }

    fn get_block(&self, _height: Height) -> Result<(Block, Hash), TestError> {
        let block = generate_block(false, &self.config);
        let block_hash = hash(&block[0..self.config.min_block_size]);
        Ok((block, block_hash))
    }

    fn sign(&self, hash: &Hash) -> Result<Signature, TestError> {
        Ok(sign(hash, &self.address))
    }

    fn check_sig(&self, signature: &Signature, _hash: &Hash) -> Result<Address, TestError> {
        let sig: &[u8] = signature;
        Ok(sig.into())
    }

    fn crypt_hash(&self, msg: &[u8]) -> Hash {
        hash(msg)
    }
}

#[derive(Clone, Debug)]
pub enum TestError {
    CheckBlockFailed,
    CheckTxsFailed,
    CheckSigFailed,
    CommitProposed,
}
