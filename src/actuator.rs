use crate::*;
use crate::{
    algorithm::{Bft, INIT_HEIGHT},
    error::BftError,
};

use crossbeam::crossbeam_channel::{unbounded, Receiver, Sender};

/// Results of Bft actuator.
pub type Result<T> = ::std::result::Result<T, BftError>;

/// A Bft Actuator
pub struct BftActuator {
    sender: Sender<BftMsg>,
    receiver: Receiver<BftMsg>,
    height: u64,
}

impl BftActuator {
    /// A function to create a new Bft actuator.
    pub fn new(address: Address) -> Self {
        let (sender, internal_receiver) = unbounded();
        let (internal_sender, receiver) = unbounded();
        Bft::start(internal_sender, internal_receiver, address);
        Self {
            sender,
            receiver,
            height: INIT_HEIGHT,
        }
    }

    /// A function to send proposal to Bft.
    pub fn send_proposal(&self, proposal: BftMsg) -> Result<()> {
        match proposal {
            BftMsg::Proposal(p) => Ok(p),
            _ => Err(BftError::MsgTypeErr),
        }
        .and_then(|p| {
            self.sender
                .send(BftMsg::Proposal(p))
                .map_err(|_| BftError::SendMsgErr)
        })
    }

    /// A function to send vote to Bft.
    pub fn send_vote(&self, vote: BftMsg) -> Result<()> {
        match vote {
            BftMsg::Vote(v) => Ok(v),
            _ => Err(BftError::MsgTypeErr),
        }
        .and_then(|v| {
            self.sender
                .send(BftMsg::Vote(v))
                .map_err(|_| BftError::SendMsgErr)
        })
    }

    /// A function to send feed to Bft.
    pub fn send_feed(&self, feed: BftMsg) -> Result<()> {
        match feed {
            BftMsg::Feed(f) => Ok(f),
            _ => Err(BftError::MsgTypeErr),
        }
        .and_then(|f| {
            self.sender
                .send(BftMsg::Feed(f))
                .map_err(|_| BftError::SendMsgErr)
        })
    }

    /// A function to send status to Bft.
    pub fn send_status(&mut self, status: BftMsg) -> Result<()> {
        match status {
            BftMsg::Status(s) => Ok(s),
            _ => Err(BftError::MsgTypeErr),
        }
        .and_then(|s| {
            if s.height >= self.height {
                self.height = s.height + 1;
            }

            self.sender
                .send(BftMsg::Status(s))
                .map_err(|_| BftError::SendMsgErr)
        })
    }

    /// A function to send verify result to Bft.
    #[cfg(feature = "verify_req")]
    pub fn send_verify(&self, verify_result: BftMsg) -> Result<()> {
        match verify_result {
            BftMsg::VerifyResp(r) => Ok(r),
            _ => Err(BftError::MsgTypeErr),
        }
        .and_then(|r| {
            self.sender
                .send(BftMsg::VerifyResp(r))
                .map_err(|_| BftError::SendMsgErr)
        })
    }

    /// A function to send pause signal to Bft.
    pub fn send_pause(&self, pause: BftMsg) -> Result<()> {
        match pause {
            BftMsg::Pause => Ok(()),
            _ => Err(BftError::MsgTypeErr),
        }
        .and_then(|_| {
            self.sender
                .send(BftMsg::Pause)
                .map_err(|_| BftError::SendMsgErr)
        })
    }

    /// A function to send start signal to Bft.
    pub fn send_start(&self, start: BftMsg) -> Result<()> {
        match start {
            BftMsg::Start => Ok(()),
            _ => Err(BftError::MsgTypeErr),
        }
        .and_then(|_| {
            self.sender
                .send(BftMsg::Start)
                .map_err(|_| BftError::SendMsgErr)
        })
    }

    /// A function to receive a Bft message via actuator.
    pub fn recv(&self) -> Result<BftMsg> {
        self.receiver
            .recv()
            .map_err(|_| BftError::RecvMsgErr)
            .and_then(|msg| match msg {
                BftMsg::Proposal(p) => Ok(BftMsg::Proposal(p)),
                BftMsg::Vote(v) => Ok(BftMsg::Vote(v)),
                BftMsg::Commit(c) => Ok(BftMsg::Commit(c)),
                _ => Err(BftError::Unreachable),
            })
    }

    /// A function to get Bft machine height.
    pub fn get_height(&self) -> u64 {
        self.height
    }
}


#[cfg(test)]
mod test {
    use super::BftActuator as Bft;
    use crate::{error::BftError, *};

    fn create_status(height: u64) -> BftMsg {
        BftMsg::Status(Status {
            height,
            interval: None,
            authority_list: vec![],
        })
    }

    fn generate_msg() -> Vec<BftMsg> {
        vec![
            BftMsg::Proposal(Proposal {
                height: 0,
                round: 0,
                content: vec![],
                lock_round: None,
                lock_votes: None,
                proposer: vec![1],
            }),
            BftMsg::Vote(Vote {
                vote_type: VoteType::Precommit,
                height: 0,
                round: 0,
                proposal: vec![],
                voter: vec![1],
            }),
            BftMsg::Feed(Feed {
                height: 0,
                proposal: vec![],
            }),
            BftMsg::Commit(Commit {
                height: 0,
                round: 0,
                proposal: vec![],
                lock_votes: vec![],
                address: vec![1],
            }),
            BftMsg::Status(Status {
                height: 0,
                interval: None,
                authority_list: vec![],
            }),
            BftMsg::Pause,
            BftMsg::Start,
        ]
    }

    #[test]
    fn test_send_proposal() {
        let actuator = Bft::new(vec![1]);
        let msg = generate_msg();

        for msg_index in 0..6 {
            let res = actuator.send_proposal(msg.get(msg_index).unwrap().to_owned());
            if msg_index == 0 {
                assert_eq!(Ok(()), res);
            } else {
                assert_eq!(Err(BftError::MsgTypeErr), res);
            }
        }
    }

    #[test]
    fn test_send_vote() {
        let actuator = Bft::new(vec![1]);
        let msg = generate_msg();

        for msg_index in 0..6 {
            let res = actuator.send_vote(msg.get(msg_index).unwrap().to_owned());
            if msg_index == 1 {
                assert_eq!(Ok(()), res);
            } else {
                assert_eq!(Err(BftError::MsgTypeErr), res);
            }
        }
    }

    #[test]
    fn test_send_feed() {
        let actuator = Bft::new(vec![1]);
        let msg = generate_msg();

        for msg_index in 0..6 {
            let res = actuator.send_feed(msg.get(msg_index).unwrap().to_owned());
            if msg_index == 2 {
                assert_eq!(Ok(()), res);
            } else {
                assert_eq!(Err(BftError::MsgTypeErr), res);
            }
        }
    }

    #[test]
    fn test_send_status() {
        let mut actuator = Bft::new(vec![1]);
        let msg = generate_msg();

        for msg_index in 0..3 {
            let res = actuator.send_status(msg.get(msg_index).unwrap().to_owned());
            if msg_index == 4 {
                assert_eq!(Ok(()), res);
            } else {
                assert_eq!(Err(BftError::MsgTypeErr), res);
            }
        }
    }

    #[test]
    fn test_send_pause() {
        let actuator = Bft::new(vec![1]);
        let msg = generate_msg();

        for msg_index in 0..6 {
            let res = actuator.send_pause(msg.get(msg_index).unwrap().to_owned());
            if msg_index == 5 {
                assert_eq!(Ok(()), res);
            } else {
                assert_eq!(Err(BftError::MsgTypeErr), res);
            }
        }
    }

    #[test]
    fn test_send_start() {
        let actuator = Bft::new(vec![1]);
        let msg = generate_msg();

        for msg_index in 0..6 {
            let res = actuator.send_start(msg.get(msg_index).unwrap().to_owned());
            if msg_index == 6 {
                assert_eq!(Ok(()), res);
            } else {
                assert_eq!(Err(BftError::MsgTypeErr), res);
            }
        }
    }

    #[test]
    fn test_height_change() {
        let height: Vec<(u64, u64)> = vec![(1, 2), (2, 3), (1, 3), (4, 5), (6, 7), (5, 7)];
        let mut actuator = Bft::new(vec![1]);
        assert_eq!(actuator.get_height(), 0);

        for h in height.into_iter() {
            if let Ok(_) = actuator.send_status(create_status(h.0)) {
                assert_eq!(actuator.get_height(), h.1);
            } else {
                panic!("Send Error!");
            }
        }
    }
}
