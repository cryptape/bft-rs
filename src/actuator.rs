use crate::*;
use crate::{
    algorithm::{Bft, INIT_HEIGHT, Step},
    error::BftError,
};

use crossbeam::crossbeam_channel::{unbounded, Receiver, Sender};

/// A Bft Actuator
pub struct BftActuator (Sender<BftMsg>);

impl BftActuator {
    /// A function to create a new Bft actuator.
    pub fn new<T: BftSupport + Send + 'static>(support: T, address: Address, wal_path: &str) -> Self {
        let (sender, internal_receiver) = unbounded();
        Bft::start(internal_receiver, support, address, wal_path);
        BftActuator(sender)
    }

    pub fn send(&self, input: BftInput) -> BftResult<()> {
        self.sender
            .send(input)
            .map_err(|_| BftError::SendMsgErr)
    }
}

#[cfg(test)]
mod test {
//    extern crate bft_rs as bft;
//    use bft::actuator::BftActuator as BFT;
//
//    struct BftFunction;
//    impl BftSupport for BftFunction {
//        fn check_block(&self, block: &[u8]) -> Result<bool, BftError>{
//            ...*
//            // 检查区块交易合法性*
//        }
//
//        fn transmit(&self, bft_msg: BftMsg) -> Result<(), BftError>{
//            ...
//            // *转发共识消息*
//        }
//
//        fn commit(&self, commit: Commit) -> Result<(), BftError>{
//            ...
//            // *处理达成共识的区块*
//        }
//    }
//    ...
//    let address = ...;
//    let wal_path = ...;
//    let bft_support = BftFunction;
//    let engine = BFT::new(bft_support, address, wal_path);
//    thread::spawn(move || loop {
//        if let Ok((key, body)) = sender.recv(){
//            match key {
//                routing_key!(Net >> SignedProposal) => {
//                    engine.send(BftInput::BftMsg(body));
//                }
//                routing_key!(Net >> RawBytes) => {
//                    engine.send(BftInput::BftMsg(body));
//                }
//                routing_key!(Chain >> RichStatus) => {
//                    let status: Status = ...  *// 转换格式*
//                    engine.send(BftInput::Status(status));
//                }
//                routing_key!(Auth >> BlockTxs) => {
//                    let feed: Feed = ...  *// 转换格式*
//                    engine.send(BftInput::Feed(feed));
//                }
//                routing_key!(Auth >> VerifyBlockResp) => {
//                    let verify_resp: VerifyResp = ...  *// 转换格式*
//                    engine.send(BftInput::VerifyResp(verify_resp));
//                }
//            }
//        }
//    });
}
