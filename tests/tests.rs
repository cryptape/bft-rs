extern crate bft_rs as bft;
extern crate crossbeam;
extern crate env_logger;
extern crate log;
extern crate rand;

use bft::algorithm::Bft;
use bft::*;
use crossbeam::crossbeam_channel::{unbounded, Receiver, Sender};
use rand::{thread_rng, Rng};

const INIT_HEIGHT: usize = 1;

#[derive(Clone, Debug, Eq, PartialEq)]
struct Node {
    height: usize,
    result: Vec<(usize, Target)>,
}

impl Node {
    fn new() -> Self {
        Node {
            height: INIT_HEIGHT,
            result: Vec::new(),
        }
    }

    fn handle_message(
        &mut self,
        msg: BftMsg,
        s_1: Sender<BftMsg>,
        s_2: Sender<BftMsg>,
        s_3: Sender<BftMsg>,
        s_result: Sender<Commit>,
    ) {
        match msg {
            BftMsg::Proposal(proposal) => {
                transmit_msg(BftMsg::Proposal(proposal), s_1, s_2, s_3);
            }
            BftMsg::Vote(vote) => {
                transmit_msg(BftMsg::Vote(vote), s_1, s_2, s_3);
            }
            BftMsg::Commit(commit) => {
                self.result
                    .push((commit.clone().height, commit.clone().proposal));
                self.height = commit.height;
                s_result.send(commit.clone()).unwrap();
                self.height += 1;
            }
            _ => println!("Invalid Message Type!"),
        }
    }
}

fn start_process(address: Address) -> (Sender<BftMsg>, Receiver<BftMsg>) {
    let (main2bft, bft4main) = unbounded();
    let (bft2main, main4bft) = unbounded();
    Bft::start(bft2main, bft4main, address);
    (main2bft, main4bft)
}

fn transmit_genesis(
    s_1: Sender<BftMsg>,
    s_2: Sender<BftMsg>,
    s_3: Sender<BftMsg>,
    s_4: Sender<BftMsg>,
) {
    s_1.send(BftMsg::Start).unwrap();
    s_2.send(BftMsg::Start).unwrap();
    s_3.send(BftMsg::Start).unwrap();
    s_4.send(BftMsg::Start).unwrap();

    let msg = BftMsg::Status(Status {
        height: INIT_HEIGHT,
        interval: None,
        authority_list: generate_auth_list(),
    });
    let feed = BftMsg::Feed(Feed {
        height: INIT_HEIGHT + 1,
        proposal: generate_proposal(),
    });

    s_1.send(msg.clone()).unwrap();
    s_2.send(msg.clone()).unwrap();
    s_3.send(msg.clone()).unwrap();
    s_4.send(msg).unwrap();

    ::std::thread::sleep(::std::time::Duration::from_micros(50));

    s_1.send(feed.clone()).unwrap();
    s_2.send(feed.clone()).unwrap();
    s_3.send(feed.clone()).unwrap();
    s_4.send(feed).unwrap();
}

fn generate_auth_list() -> Vec<Address> {
    vec![vec![0], vec![1], vec![2], vec![3]]
}

fn generate_proposal() -> Target {
    let mut proposal = vec![1, 2, 3];
    let mut rng = thread_rng();

    for ii in proposal.iter_mut() {
        *ii = rng.gen();
    }
    proposal
}

fn transmit_msg(msg: BftMsg, s_1: Sender<BftMsg>, s_2: Sender<BftMsg>, s_3: Sender<BftMsg>) {
    s_1.send(msg.clone()).unwrap();
    s_2.send(msg.clone()).unwrap();
    s_3.send(msg).unwrap();
}

mod integration_cases;
