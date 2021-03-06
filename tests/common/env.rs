extern crate bft_rs;

use self::bft_rs::timer::{GetInstant, WaitTimer};
use self::bft_rs::{Address, Hash, Height};
use super::config::{Config, LIVENESS_TICK};
use super::support::Support;
use super::utils::*;
use bft_rs::{BftActuator, BftMsg, Commit, Node, Status};
use crossbeam::crossbeam_channel::{select, unbounded, Receiver, RecvError, Sender};
#[allow(unused_imports)]
use log::{info, log};
use lru_cache::LruCache;
use std::cmp::{Ord, Ordering, PartialOrd};
use std::collections::HashMap;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

pub struct Env {
    pub config: Config,
    pub wal_dir: &'static str,
    pub live_nodes: HashMap<Address, Box<BftActuator>>,
    pub byzantine_nodes: Vec<Address>,
    pub msg_recv: Receiver<(BftMsg, Address)>,
    pub msg_send: Sender<(BftMsg, Address)>,
    pub commit_recv: Receiver<(Commit, Address)>,
    pub commit_send: Sender<(Commit, Address)>,
    pub test4timer: Receiver<Event>,
    pub test2timer: Sender<Event>,
    pub authority_list: Vec<Node>,
    pub interval: Option<u64>,
    pub status: Status,
    pub status_list: LruCache<Height, Status>,
    //    pub old_status: Option<Status>,
    pub last_reach_consensus_time: Instant,
    pub commits: LruCache<Height, Hash>,
    pub nodes_height: HashMap<Address, Height>,
}

impl Env {
    pub fn new(config: Config, nodes_num: usize, wal_dir: &'static str) -> Env {
        let mut live_nodes = HashMap::new();
        let mut nodes_height = HashMap::new();
        let mut authority_list = vec![];
        let (msg_send, msg_recv) = unbounded();
        let (commit_send, commit_recv) = unbounded();
        for i in 0..nodes_num {
            let address = generate_address();

            let node = Node {
                address: address.clone(),
                proposal_weight: 1u32,
                vote_weight: 1u32,
            };
            authority_list.push(node);

            let node_support = Support {
                config,
                address: address.clone(),
                msg_send: msg_send.clone(),
                commit_send: commit_send.clone(),
            };
            let wal_path = format!("{}{}", wal_dir, i);
            let actuator = BftActuator::new(Arc::new(node_support), address.clone(), &wal_path);
            live_nodes.insert(address.clone(), Box::new(actuator));
            nodes_height.insert(address, 0);
        }

        let interval = Some(3000);

        let status = Status {
            height: 0u64,
            interval,
            authority_list: authority_list.clone(),
        };

        let mut status_list = LruCache::new(16);
        status_list.insert(0u64, status.clone());

        let (test2timer, timer4test) = unbounded();
        let (timer2test, test4timer) = unbounded();
        let _timer_thread = thread::Builder::new()
            .name("test_timer".to_string())
            .spawn(move || {
                let timer = WaitTimer::new(timer2test, timer4test);
                timer.start();
            })
            .unwrap();

        Env {
            config,
            wal_dir,
            live_nodes,
            byzantine_nodes: vec![],
            msg_recv,
            msg_send,
            commit_recv,
            commit_send,
            test4timer,
            test2timer,
            authority_list,
            interval,
            status,
            status_list,
            last_reach_consensus_time: Instant::now(),
            commits: LruCache::new(16),
            nodes_height,
        }
    }

    pub fn run(&mut self, stop_height: u64) {
        let event = Event {
            process_time: Instant::now(),
            to: Address::default(),
            content: Content::Sync,
        };
        self.test2timer.send(event).unwrap();

        loop {
            let mut get_msg = Err(RecvError);
            let mut get_commit = Err(RecvError);
            let mut get_timer = Err(RecvError);

            select! {
                recv(self.msg_recv) -> msg => get_msg = msg,
                recv(self.commit_recv) -> msg => get_commit = msg,
                recv(self.test4timer) -> msg => get_timer = msg,
            }

            if let Ok((msg, from)) = get_msg {
                self.live_nodes.iter().for_each(|(address, _)| {
                    if address != &from {
                        let delay = message_delay(&self.config);
                        let event = Event {
                            process_time: Instant::now() + delay,
                            to: address.clone(),
                            content: Content::Msg(msg.clone()),
                        };
                        self.test2timer.send(event).unwrap();
                    }
                });
            }
            if let Ok((commit, sender)) = get_commit {
                let ch = commit.height;
                let sh = self.status.height;
                if ch < sh {
                    info!("node {:?} reach old consensus in height {}", sender, ch);
                    self.check_consistency(&commit);

                    let delay = sync_delay(sh - ch, &self.config);
                    let event = Event {
                        process_time: Instant::now() + delay,
                        to: sender,
                        content: Content::Status(self.status.clone()),
                    };
                    self.test2timer.send(event).unwrap();
                } else if ch == sh {
                    info!("node {:?} reach consensus in height {}", sender, ch);
                    self.check_consistency(&commit);

                    let delay = commit_delay(&self.config);
                    let event = Event {
                        process_time: Instant::now() + delay,
                        to: sender,
                        content: Content::Status(self.status.clone()),
                    };
                    self.test2timer.send(event).unwrap();
                } else if ch == sh + 1 {
                    if ch == stop_height {
                        self.live_nodes
                            .iter()
                            .for_each(|(_, actuator)| actuator.send(BftMsg::Kill).unwrap());
                        break;
                    }
                    info!(
                        "node {:?} first reach new consensus in height {}",
                        sender, ch
                    );
                    self.commits.insert(ch, hash(&commit.block));
                    let delay = commit_delay(&self.config);
                    let status = self.create_status(ch);
                    let event = Event {
                        process_time: Instant::now() + delay,
                        to: sender,
                        content: Content::Status(status),
                    };
                    self.test2timer.send(event).unwrap();

                    self.last_reach_consensus_time = Instant::now();
                    let event = Event {
                        process_time: Instant::now() + LIVENESS_TICK,
                        to: Address::default(),
                        content: Content::LivenessTimeout(ch, 1),
                    };
                    self.test2timer.send(event).unwrap();
                } else {
                    panic!(
                        "jump height from {} to {}",
                        self.status.height, commit.height
                    );
                }
            }
            if let Ok(event) = get_timer {
                let content = event.content;
                let to = event.to;
                match content {
                    Content::Msg(bft_msg) => {
                        if let Some(actuator) = self.live_nodes.get(&to) {
                            actuator.send(bft_msg).unwrap();
                        }
                    }
                    Content::Status(status) => {
                        self.nodes_height.insert(to.clone(), status.height);
                        if let Some(actuator) = self.live_nodes.get(&to) {
                            actuator.send(BftMsg::Status(status)).unwrap();
                        }
                    }
                    Content::LivenessTimeout(height, n) => {
                        if height == self.status.height {
                            info!(
                                "WARNING! no node reach consensus in last {} minutes at height {}",
                                n, height
                            );
                            let event = Event {
                                process_time: Instant::now() + LIVENESS_TICK,
                                to: Address::default(),
                                content: Content::LivenessTimeout(height, n + 1),
                            };
                            self.test2timer.send(event).unwrap();
                        }
                    }
                    Content::Sync => {
                        self.corrupt();
                        self.try_sync();
                    }
                    Content::Corrupt => {
                        self.byzantine_nodes.push(to);
                    }
                    Content::Start(i) => {
                        let actuator = self.generate_node(to.clone(), i);
                        info!("Node {:?} is started", to);
                        self.live_nodes.insert(to, Box::new(actuator));
                    }
                    Content::Stop => {
                        let actuator = self.live_nodes.remove(&to).unwrap();
                        actuator.send(BftMsg::Kill).unwrap();
                        info!("Node {:?} is stopped", to);
                    }
                }
            }
        }

        info!("Successfully pass the test!");
    }

    pub fn generate_node(&self, address: Address, i: usize) -> BftActuator {
        let node_support = Support {
            config: self.config,
            address: address.clone(),
            msg_send: self.msg_send.clone(),
            commit_send: self.commit_send.clone(),
        };
        let wal_path = format!("{}{}", self.wal_dir, i);
        BftActuator::new(Arc::new(node_support), address, &wal_path)
    }

    pub fn check_consistency(&mut self, commit: &Commit) {
        if self.commits.contains_key(&commit.height) {
            let hash = &hash(&commit.block);
            let compare = self.commits.get_mut(&commit.height).unwrap();
            if hash != compare {
                panic!("consistency is broken of commit {:?}", commit);
            }
        } else {
            info!("too old commit, failed to check consistency!");
        }
    }

    pub fn create_status(&mut self, height: u64) -> Status {
        let status = Status {
            height,
            authority_list: self.authority_list.clone(),
            interval: self.interval,
        };
        self.status = status.clone();
        self.status_list.insert(height, status.clone());
        status
    }

    pub fn try_sync(&mut self) {
        let live_honest_heights: HashMap<&Address, &Height> = self
            .nodes_height
            .iter()
            .filter(|(address, _)| {
                self.live_nodes.contains_key(*address) && !self.byzantine_nodes.contains(*address)
            })
            .collect();
        if let Some(max_height) = live_honest_heights.values().max() {
            let result = self.status_list.get_mut(*max_height).cloned();
            if let Some(status) = result {
                self.nodes_height.iter().for_each(|(address, height)| {
                    if height < *max_height || *height == 0 {
                        let delay = sync_delay(*max_height - height, &self.config);
                        let event = Event {
                            process_time: Instant::now() + delay,
                            to: address.clone(),
                            content: Content::Status(status.clone()),
                        };
                        self.test2timer.send(event).unwrap();
                    }
                });
            }
        }

        let event = Event {
            process_time: Instant::now() + Duration::from_millis(self.config.sync_trigger_duration),
            to: Address::default(),
            content: Content::Sync,
        };
        self.test2timer.send(event).unwrap();
    }

    pub fn corrupt(&self) {
        self.live_nodes.iter().for_each(|(address, actuator)| {
            if self.byzantine_nodes.contains(address) {
                actuator.send(BftMsg::Corrupt).unwrap();
            }
        });
    }

    pub fn get_node_address(&self, i: usize) -> Option<Address> {
        self.authority_list
            .get(i)
            .and_then(|node| Some(node.address.clone()))
    }

    pub fn set_node(&mut self, i: usize, content: Content, duration: Duration) {
        if let Some(address) = self.get_node_address(i) {
            let event = Event {
                process_time: Instant::now() + duration,
                to: address.clone(),
                content,
            };
            self.test2timer.send(event).unwrap();
        }
    }
}

#[derive(Debug, Clone)]
pub struct Event {
    process_time: Instant,
    to: Address,
    content: Content,
}

impl PartialEq for Event {
    fn eq(&self, other: &Self) -> bool {
        self.process_time == other.process_time
    }
}
impl Eq for Event {}

impl PartialOrd for Event {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.process_time.partial_cmp(&other.process_time)
    }
}

impl Ord for Event {
    fn cmp(&self, other: &Self) -> Ordering {
        self.process_time.cmp(&other.process_time)
    }
}

impl GetInstant for Event {
    fn get_instant(&self) -> Instant {
        self.process_time
    }
}

#[derive(Debug, Clone)]
pub enum Content {
    Msg(BftMsg),
    Status(Status),
    LivenessTimeout(u64, usize), // Duration, count
    Sync,
    Stop,
    Start(usize),
    Corrupt,
}
