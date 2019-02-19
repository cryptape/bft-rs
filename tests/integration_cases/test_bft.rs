// CITA
// Copyright 2016-2019 Cryptape Technologies LLC.

// This program is free software: you can redistribute it
// and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation,
// either version 3 of the License, or (at your option) any
// later version.

// This program is distributed in the hope that it will be
// useful, but WITHOUT ANY WARRANTY; without even the implied
// warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR
// PURPOSE. See the GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <http://www.gnu.org/licenses/>.
use bft::*;
// use bft::algorithm::Bft;
use crossbeam::Sender;
use rand::{thread_rng, Rng};

use std::thread;
use std::time::{Duration, Instant};
use std::vec;

use start_process;

const INIT_HEIGHT: usize = 1;
const MAX_TEST_HEIGHT: usize = 100;

struct Node {
    height: usize,
    result: Vec<(usize, Target)>,
    ins: Instant,
}

impl Node {
    fn new() -> Self {
        Node {
            height: INIT_HEIGHT,
            result: Vec::new(),
            ins: Instant::now(),
        }
    }

    fn handle_message(
        &mut self,
        msg: BftMsg,
        auth_list: Vec<Address>,
        address: i8,
        s_1: Sender<BftMsg>,
        s_2: Sender<BftMsg>,
        s_3: Sender<BftMsg>,
        s_self: Sender<BftMsg>,
    ) {
        match msg {
            BftMsg::Proposal(proposal) => {
                transmit_msg(BftMsg::Proposal(proposal), s_1, s_2, s_3);
            }
            BftMsg::Vote(vote) => {
                transmit_msg(BftMsg::Vote(vote), s_1, s_2, s_3);
            }
            BftMsg::Commit(commit) => {
                self.result.push((commit.height, commit.proposal));
                self.height = commit.height;

                s_self
                    .send(BftMsg::RichStatus(RichStatus {
                        height: commit.height,
                        interval: None,
                        authority_list: auth_list,
                    }))
                    .unwrap();

                println!(
                    "Node {:?}, height {:?}, consensus time {:?}",
                    address,
                    commit.height,
                    Instant::now() - self.ins
                );
                self.ins = Instant::now();
                thread::sleep(Duration::from_micros(20));
                self.height += 1;

                s_self
                    .send(BftMsg::Feed(Feed {
                        height: commit.height + 1,
                        proposal: generate_proposal(),
                    }))
                    .unwrap();
            }
            _ => println!("Invalid Message Type!"),
        }
    }
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

fn transmit_genesis(
    s_1: Sender<BftMsg>,
    s_2: Sender<BftMsg>,
    s_3: Sender<BftMsg>,
    s_4: Sender<BftMsg>,
) {
    let msg = BftMsg::RichStatus(RichStatus {
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

    thread::sleep(Duration::from_micros(50));

    s_1.send(feed.clone()).unwrap();
    s_2.send(feed.clone()).unwrap();
    s_3.send(feed.clone()).unwrap();
    s_4.send(feed).unwrap();
}

#[test]
fn test_bft() {
    let (send_node_0, recv_node_0) = start_process(vec![0]);
    let (send_node_1, recv_node_1) = start_process(vec![1]);
    let (send_node_2, recv_node_2) = start_process(vec![2]);
    let (send_node_3, recv_node_3) = start_process(vec![3]);

    // simulate the message from executor when executed genesis block
    transmit_genesis(
        send_node_0.clone(),
        send_node_1.clone(),
        send_node_2.clone(),
        send_node_3.clone(),
    );

    // this is the thread of node 0
    let send_0 = send_node_0.clone();
    let send_1 = send_node_1.clone();
    let send_2 = send_node_2.clone();
    let send_3 = send_node_3.clone();
    let mut node_0 = Node::new();

    thread::spawn(move || loop {
        if let Ok(recv) = recv_node_0.recv() {
            node_0.handle_message(
                recv,
                generate_auth_list(),
                0,
                send_1.clone(),
                send_2.clone(),
                send_3.clone(),
                send_0.clone(),
            );
            println!("{:?}", node_0.height);
        }

        if node_0.height == MAX_TEST_HEIGHT {
            break;
        }
    });

    // this is the thread of node 1
    let send_0 = send_node_0.clone();
    let send_1 = send_node_1.clone();
    let send_2 = send_node_2.clone();
    let send_3 = send_node_3.clone();
    let mut node_1 = Node::new();

    thread::spawn(move || loop {
        if let Ok(recv) = recv_node_1.recv() {
            node_1.handle_message(
                recv,
                generate_auth_list(),
                0,
                send_0.clone(),
                send_2.clone(),
                send_3.clone(),
                send_1.clone(),
            );
        }
        if node_1.height == MAX_TEST_HEIGHT {
            break;
        }
    });

    // this is the thread of node 2
    let send_0 = send_node_0.clone();
    let send_1 = send_node_1.clone();
    let send_2 = send_node_2.clone();
    let send_3 = send_node_3.clone();
    let mut node_2 = Node::new();

    thread::spawn(move || loop {
        if let Ok(recv) = recv_node_2.recv() {
            node_2.handle_message(
                recv,
                generate_auth_list(),
                0,
                send_1.clone(),
                send_0.clone(),
                send_3.clone(),
                send_2.clone(),
            );
        }

        if node_2.height == MAX_TEST_HEIGHT {
            break;
        }
    });

    // this is the thread of node 3
    let send_0 = send_node_0.clone();
    let send_1 = send_node_1.clone();
    let send_2 = send_node_2.clone();
    let send_3 = send_node_3.clone();
    let mut node_3 = Node::new();

    thread::spawn(move || loop {
        if let Ok(recv) = recv_node_3.recv() {
            node_3.handle_message(
                recv,
                generate_auth_list(),
                0,
                send_1.clone(),
                send_2.clone(),
                send_0.clone(),
                send_3.clone(),
            );
        }

        if node_3.height == MAX_TEST_HEIGHT {
            break;
        }
    });
}
