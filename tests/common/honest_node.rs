extern crate bft_rs;

use self::bft_rs::{Address, BftMsg, BftSupport, Commit, Signature as BftSig, Status};
use super::utils::*;
use crossbeam::crossbeam_channel::Sender;
use std::thread;

pub struct HonestNode {
    pub address: Vec<u8>,
    pub msg_send: Sender<BftMsg>,
    pub commit_send: Sender<(Commit, Address)>,
}

impl BftSupport for HonestNode {
    type Error = TestError;
    fn check_block(&self, block: &[u8], _height: u64) -> Result<(), TestError> {
        if check_block_result(block) {
            Ok(())
        } else {
            Err(TestError::CheckBlockFailed)
        }
    }

    fn check_txs(
        &self,
        _block: &[u8],
        _signed_proposal_hash: &[u8],
        _height: u64,
        _round: u64,
    ) -> Result<(), TestError> {
        let delay = check_txs_delay();
        thread::sleep(delay);
        if check_txs_result() {
            Ok(())
        } else {
            Err(TestError::CheckTxsFailed)
        }
    }

    fn transmit(&self, msg: BftMsg) {
        self.msg_send.send(msg).unwrap();
    }

    fn commit(&self, commit: Commit) -> Result<Status, TestError> {
        let address = self.address.clone();
        self.commit_send.send((commit, address)).unwrap();
        Err(TestError::CommitProposed)
    }

    fn get_block(&self, _height: u64) -> Result<Vec<u8>, TestError> {
        Ok(generate_block(false))
    }

    fn sign(&self, hash: &[u8]) -> Result<BftSig, TestError> {
        Ok(sign(hash, &self.address))
    }

    fn check_sig(&self, signature: &[u8], _hash: &[u8]) -> Result<Address, TestError> {
        Ok(signature.to_vec())
    }

    fn crypt_hash(&self, msg: &[u8]) -> Vec<u8> {
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
