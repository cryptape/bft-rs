use bft::*;
use crossbeam::crossbeam_channel::unbounded;
use env_logger::Builder;
use env_logger::Target as LogTarget;
use log::LevelFilter;
use rand::{thread_rng, Rng};

use crate::*;

use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const MAX_TEST_HEIGHT: usize = 10000;

fn random_offline() -> bool {
    let mut rng = thread_rng();
    let x: f64 = rng.gen_range(0 as f64, 1 as f64);
    // the offline probablity is 0.0125
    if x < 0.0125 {
        return true;
    }
    false
}

fn is_success(result: Vec<Target>) -> bool {
    let mut result_hashmap: HashMap<Target, u8> = HashMap::new();
    for ii in result.into_iter() {
        let counter = result_hashmap.entry(ii).or_insert(0);
        *counter += 1;
    }
    for (_, count) in result_hashmap.into_iter() {
        if count >= 3 {
            return true;
        }
    }
    false
}

#[test]
fn test_random_offline() {
    Builder::new()
        .default_format_timestamp_nanos(true)
        .format(|buf, record| {
            writeln!(
                buf,
                "{} - {} - {}",
                thread::current().name().clone().unwrap(),
                record.level(),
                record.args()
            )
        })
        .filter(None, LevelFilter::Trace)
        .target(LogTarget::Stdout)
        .init();

    let (send_node_0, recv_node_0) = start_process(vec![0]);
    let (send_node_1, recv_node_1) = start_process(vec![1]);
    let (send_node_2, recv_node_2) = start_process(vec![2]);
    let (send_node_3, recv_node_3) = start_process(vec![3]);
    let (send_result, recv_result) = unbounded();

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
    let send_r = send_result.clone();
    let node_0 = Arc::new(Mutex::new(Node::new()));
    let node_0_clone = node_0.clone();

    let thread_0 = thread::Builder::new()
        .name("node_0".to_string())
        .spawn(move || loop {
            if let Ok(recv) = recv_node_0.recv() {
                node_0_clone.lock().unwrap().handle_message(
                    recv,
                    send_1.clone(),
                    send_2.clone(),
                    send_3.clone(),
                    send_r.clone(),
                );
            }

            if random_offline() {
                let mut rng = thread_rng();
                // offline for t secends
                let t: u64 = rng.gen_range(500, 3000);
                println!("!!! Node 0 offline {:?} sec", t as f64 / 1000 as f64);

                send_0.send(BftMsg::Pause).unwrap();
                thread::sleep(Duration::from_millis(t));
                send_0.send(BftMsg::Start).unwrap();
            }

            if node_0_clone.lock().unwrap().height == MAX_TEST_HEIGHT {
                ::std::process::exit(0);
            }
        })
        .unwrap();

    // this is the thread of node 1
    let send_0 = send_node_0.clone();
    let send_1 = send_node_1.clone();
    let send_2 = send_node_2.clone();
    let send_3 = send_node_3.clone();
    let send_r = send_result.clone();
    let node_1 = Arc::new(Mutex::new(Node::new()));
    let node_1_clone = node_1.clone();

    let thread_1 = thread::Builder::new()
        .name("node_1".to_string())
        .spawn(move || loop {
            if let Ok(recv) = recv_node_1.recv() {
                node_1_clone.lock().unwrap().handle_message(
                    recv,
                    send_0.clone(),
                    send_2.clone(),
                    send_3.clone(),
                    send_r.clone(),
                );
            }

            if random_offline() {
                let mut rng = thread_rng();
                // offline for t secends
                let t: u64 = rng.gen_range(500, 3000);
                println!("!!! Node 1 offline {:?} sec", t as f64 / 1000 as f64);

                send_1.send(BftMsg::Pause).unwrap();
                thread::sleep(Duration::from_millis(t));
                send_1.send(BftMsg::Start).unwrap();
            }

            if node_1_clone.lock().unwrap().height == MAX_TEST_HEIGHT {
                ::std::process::exit(0);
            }
        })
        .unwrap();

    // this is the thread of node 2
    let send_0 = send_node_0.clone();
    let send_1 = send_node_1.clone();
    let send_2 = send_node_2.clone();
    let send_3 = send_node_3.clone();
    let send_r = send_result.clone();
    let node_2 = Arc::new(Mutex::new(Node::new()));
    let node_2_clone = node_2.clone();

    let thread_2 = thread::Builder::new()
        .name("node_2".to_string())
        .spawn(move || loop {
            if let Ok(recv) = recv_node_2.recv() {
                node_2_clone.lock().unwrap().handle_message(
                    recv,
                    send_1.clone(),
                    send_0.clone(),
                    send_3.clone(),
                    send_r.clone(),
                );
            }

            if random_offline() {
                let mut rng = thread_rng();
                // offline for t secends
                let t: u64 = rng.gen_range(500, 3000);
                println!("!!! Node 2 offline {:?} sec", t as f64 / 1000 as f64);

                send_2.send(BftMsg::Pause).unwrap();
                thread::sleep(Duration::from_millis(t));
                send_2.send(BftMsg::Start).unwrap();
            }

            if node_2_clone.lock().unwrap().height == MAX_TEST_HEIGHT {
                ::std::process::exit(0);
            }
        })
        .unwrap();

    // this is the thread of node 3
    let send_0 = send_node_0.clone();
    let send_1 = send_node_1.clone();
    let send_2 = send_node_2.clone();
    let send_3 = send_node_3.clone();
    let send_r = send_result.clone();
    let node_3 = Arc::new(Mutex::new(Node::new()));
    let node_3_clone = node_3.clone();

    let thread_3 = thread::Builder::new()
        .name("node_3".to_string())
        .spawn(move || loop {
            if let Ok(recv) = recv_node_3.recv() {
                node_3_clone.lock().unwrap().handle_message(
                    recv,
                    send_1.clone(),
                    send_2.clone(),
                    send_0.clone(),
                    send_r.clone(),
                );
            }

            if random_offline() {
                let mut rng = thread_rng();
                // offline for t secends
                let t: u64 = rng.gen_range(500, 3000);
                println!("!!! Node 3 offline {:?} sec", t as f64 / 1000 as f64);

                send_3.send(BftMsg::Pause).unwrap();
                thread::sleep(Duration::from_millis(t));
                send_3.send(BftMsg::Start).unwrap();
            }

            if node_3_clone.lock().unwrap().height == MAX_TEST_HEIGHT {
                ::std::process::exit(0);
            }
        })
        .unwrap();

    let send_0 = send_node_0.clone();
    let send_1 = send_node_1.clone();
    let send_2 = send_node_2.clone();
    let send_3 = send_node_3.clone();
    let sender = vec![
        send_0.clone(),
        send_1.clone(),
        send_2.clone(),
        send_3.clone(),
    ];

    let thread_commit = thread::Builder::new()
        .name("commit_thread".to_string())
        .spawn(move || {
            let mut chain_height: usize = 2;
            let mut result: Vec<Target> = Vec::new();
            let mut node_0_height = 0;
            let mut node_1_height = 0;
            let mut node_2_height = 0;
            let mut node_3_height = 0;
            let mut now = Instant::now();
            let mut height_result: HashMap<usize, Target> = HashMap::new();

            loop {
                if let Ok(recv) = recv_result.recv() {
                    if recv.clone().address == vec![0] {
                        node_0_height = recv.height + 1;
                    } else if recv.clone().address == vec![1] {
                        node_1_height = recv.height + 1;
                    } else if recv.clone().address == vec![2] {
                        node_2_height = recv.height + 1;
                    } else if recv.clone().address == vec![3] {
                        node_3_height = recv.height + 1;
                    }

                    if height_result.contains_key(&recv.height) {
                        if &recv.proposal != height_result.get(&recv.height).unwrap() {
                            println!("Fork!!!");
                            ::std::process::exit(1);
                        }
                    } else {
                        height_result.insert(recv.height, recv.proposal.clone());
                    }

                    sender[recv.address[0].clone() as usize]
                        .send(BftMsg::Status(Status {
                            height: recv.height.clone(),
                            interval: None,
                            authority_list: generate_auth_list(),
                        }))
                        .unwrap();
                    sender[recv.address[0].clone() as usize]
                        .send(BftMsg::Feed(Feed {
                            height: recv.height.clone() + 1,
                            proposal: generate_proposal(),
                        }))
                        .unwrap();
                }
                if result.clone().len() >= 3 {
                    if is_success(result.clone()) {
                        println!(
                            "Consensus success at height {:?}, with {:?}",
                            chain_height,
                            Instant::now() - now
                        );
                        result.clear();
                        chain_height += 1;
                        now = Instant::now();
                    } else {
                        ::std::process::exit(1);
                    }
                }

                // simulate sync proc
                if node_0_height < chain_height - 1 {
                    println!("Sync node 0 to height {:?}", chain_height);
                    send_0
                        .send(BftMsg::Status(Status {
                            height: chain_height - 1,
                            interval: None,
                            authority_list: generate_auth_list(),
                        }))
                        .unwrap();
                }
                if node_1_height < chain_height - 1 {
                    println!("Sync node 1 to height {:?}", chain_height);
                    send_1
                        .send(BftMsg::Status(Status {
                            height: chain_height - 1,
                            interval: None,
                            authority_list: generate_auth_list(),
                        }))
                        .unwrap();
                }
                if node_2_height < chain_height - 1 {
                    println!("Sync node 2 to height {:?}", chain_height);
                    send_2
                        .send(BftMsg::Status(Status {
                            height: chain_height - 1,
                            interval: None,
                            authority_list: generate_auth_list(),
                        }))
                        .unwrap();
                }
                if node_3_height < chain_height - 1 {
                    println!("Sync node 3 to height {:?}", chain_height);
                    send_3
                        .send(BftMsg::Status(Status {
                            height: chain_height - 1,
                            interval: None,
                            authority_list: generate_auth_list(),
                        }))
                        .unwrap();
                }
            }
        })
        .unwrap();

    thread_0.join();
    thread_1.join();
    thread_2.join();
    thread_3.join();
    thread_commit.join();
}
