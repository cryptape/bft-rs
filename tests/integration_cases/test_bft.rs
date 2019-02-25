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
use crossbeam::Sender;

use crate::*;

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use std::vec;

const INIT_HEIGHT: usize = 1;
const MAX_TEST_HEIGHT: usize = 50;

#[derive(Clone, Debug, Eq, PartialEq)]
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
                println!("Node {:?} proposal {:?}", proposal.proposer, proposal.content);
                transmit_msg(BftMsg::Proposal(proposal), s_1, s_2, s_3);
            }
            BftMsg::Vote(vote) => {
                println!("Node {:?} vote {:?}", vote.voter, vote.proposal);
                transmit_msg(BftMsg::Vote(vote), s_1, s_2, s_3);
            }
            BftMsg::Commit(commit) => {
                self.result.push((commit.clone().height, commit.clone().proposal));
                self.height = commit.height;
                s_self
                    .send(BftMsg::Status(Status {
                        height: commit.height,
                        interval: None,
                        authority_list: auth_list,
                    }))
                    .unwrap();

                println!(
                    "Node {:?}, height {:?}, result {:?}, consensus time {:?}",
                    address,
                    commit.clone().height,
                    commit.proposal,
                    Instant::now() - self.ins
                );

                self.ins = Instant::now();
                // thread::sleep(Duration::from_micros(10));
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

fn transmit_genesis(
    s_1: Sender<BftMsg>,
    s_2: Sender<BftMsg>,
    s_3: Sender<BftMsg>,
    s_4: Sender<BftMsg>,
) {
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
    let node_0 = Arc::new(Mutex::new(Node::new()));
    let node_0_clone = node_0.clone();

    let thread_0 = thread::spawn(move || loop {
        if let Ok(recv) = recv_node_0.recv() {
            node_0_clone.lock().unwrap().handle_message(
                recv,
                generate_auth_list(),
                0,
                send_1.clone(),
                send_2.clone(),
                send_3.clone(),
                send_0.clone(),
            );
            // println!("{:?}", node_0.lock().unwrap().height);
        }
        if node_0_clone.lock().unwrap().height == MAX_TEST_HEIGHT {
            ::std::process::exit(0);
        }
    });

    // this is the thread of node 1
    let send_0 = send_node_0.clone();
    let send_1 = send_node_1.clone();
    let send_2 = send_node_2.clone();
    let send_3 = send_node_3.clone();
    let node_1 = Arc::new(Mutex::new(Node::new()));
    let node_1_clone = node_1.clone();

    let thread_1 = thread::spawn(move || loop {
        if let Ok(recv) = recv_node_1.recv() {
            node_1_clone.lock().unwrap().handle_message(
                recv,
                generate_auth_list(),
                1,
                send_0.clone(),
                send_2.clone(),
                send_3.clone(),
                send_1.clone(),
            );
        }
        if node_1_clone.lock().unwrap().height == MAX_TEST_HEIGHT {
            ::std::process::exit(0);
        }
    });

    // this is the thread of node 2
    let send_0 = send_node_0.clone();
    let send_1 = send_node_1.clone();
    let send_2 = send_node_2.clone();
    let send_3 = send_node_3.clone();
    let node_2 = Arc::new(Mutex::new(Node::new()));
    let node_2_clone = node_2.clone();

    let thread_2 = thread::spawn(move || loop {
        if let Ok(recv) = recv_node_2.recv() {
            node_2_clone.lock().unwrap().handle_message(
                recv,
                generate_auth_list(),
                2,
                send_1.clone(),
                send_0.clone(),
                send_3.clone(),
                send_2.clone(),
            );
        }

        if node_2_clone.lock().unwrap().height == MAX_TEST_HEIGHT {
            ::std::process::exit(0);
        }
    });

    // this is the thread of node 3
    let send_0 = send_node_0.clone();
    let send_1 = send_node_1.clone();
    let send_2 = send_node_2.clone();
    let send_3 = send_node_3.clone();
    let node_3 = Arc::new(Mutex::new(Node::new()));
    let node_3_clone = node_3.clone();

    let thread_3 = thread::spawn(move || loop {
        if let Ok(recv) = recv_node_3.recv() {
            node_3_clone.lock().unwrap().handle_message(
                recv,
                generate_auth_list(),
                3,
                send_1.clone(),
                send_2.clone(),
                send_0.clone(),
                send_3.clone(),
            );
        }

        if node_3_clone.lock().unwrap().height == MAX_TEST_HEIGHT {
            ::std::process::exit(0);
        }
    });

    thread_0.join();
    thread_1.join();
    thread_2.join();
    thread_3.join();
}
