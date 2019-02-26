extern crate bft_rs as bft;
extern crate crossbeam;
extern crate csv;
extern crate rand;

use bft::algorithm::Bft;
use bft::*;
use crossbeam::crossbeam_channel::{unbounded, Receiver, Sender};
use rand::{thread_rng, Rng};

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
