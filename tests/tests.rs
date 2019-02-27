extern crate bft_rs as bft;
extern crate crossbeam;
extern crate csv;
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
                println!(
                    "Node {:?} proposal {:?}",
                    proposal.proposer, proposal.content
                );
                transmit_msg(BftMsg::Proposal(proposal), s_1, s_2, s_3);
            }
            BftMsg::Vote(vote) => {
                println!(
                    "Node {:?}, height {:?}, round {:?}, {:?} vote {:?}",
                    vote.voter, vote.height, vote.round, vote.vote_type, vote.proposal
                );
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
