# BFT

An efficient and stable Rust library of BFT protocol for distributed system.

## What is BFT?

BFT(Byzantine Fault Tolerance) comprise a class of consensus algorithms that achieve byzantine fault tolerance. BFT can guarantee liveness and safety for a distributed system where there are not more than 33% malicious byzantine nodes, and thus BFT is often used in the blockchain network.

## BFT Protocol

### Protocol

BFT is a State Machine Replication algorithm, and some states are shown below:

1. The three states protocol

```
    NewHeight -> (Propose -> Prevote -> Precommit)+ -> Commit -> NewHeight -> ...
```

2. The three states protocol in a height

```
                            +-------------------------------------+
                            |                                     | (Wait new block)
                            v                                     |
                      +-----------+                         +-----+-----+
         +----------> |  Propose  +--------------+          | NewHeight |
         |            +-----------+              |          +-----------+
         |                                       |                ^
         | (Else)                                |                |
         |                                       v                |
   +-----+-----+                           +-----------+          |
   | Precommit |  <------------------------+  Prevote  |          | (Wait RichStatus)
   +-----+-----+                           +-----------+          |
         |                                                        |
         | (When +2/3 Precommits for the block found)             |
         v                                                        |
   +--------------------------------------------------------------+-----+
   |  Commit                                                            |
   |                                                                    |
   |  * Generate Proof;                                                 |
   |  * Set CommitTime = now;                                           |
   +--------------------------------------------------------------------+
```

### Architecture

A complete BFT model consists of 4 essential parts:

1. Consensus Module, the consensus algorithm module includes signature verification, proof generation, version check, etc.;

2. State Machine, the BFT state machine is focused on consensus proposal;

3. Transport Module, the network for consensus module to communicate with other modules;

4. Wal Module, the place saving BFT logs.

**NOTICE**: The bft-rs only provides a basic BFT state machine and does not support the advanced functions such as signature verification, proof generation, compact block, etc. These functions are in consensus module rather than bft-rs library.

## Feature
The bft-rs provides `verify_req` feature to verify transcation after received a proposal. BFT state machine will check the verify result of the proposal before `Precommit` step. If it has not received the result of the proposal yet, it will wait for an extra 1/2 of the consensus duration.

## Interface

If bft-rs works correctly, it needs to receive 4 types of message: `Proposal`, `Vote`, `Feed`, `Status`. And  bft-rs can send 3 types of message: `Proposal`, `Vote`, `Commit`. Besides, bft-rs also provides `Stop` and `Start` message that can control state machine stop or go on. These types of messages consist of the `enum BftMsg`:

```rust
enum BftMsg {
    Proposal(Proposal),
    Vote(Vote),
    Feed(Feed),
    Status(Status),
    Commit(Commit),

    #[cfg(feature = "verify_req")]
    VerifyResp(VerifyResp),
    Pause,
    Start,
}
```

For detailed introduction, click [here](src/lib.rs).

## Usage

First, add bft-rs and crossbeam to your `Cargo.toml`:

```rust
[dependencies]
bft-rs = { git = "https://github.com/cryptape/bft-rs.git", branch = "develop" }
```

If you want to use `verify_req` feature, needs to add following codes:

```rust
[features]
verify_req = ["bft-rs/verify_req"]
```

Second, add BFT and channel to your crate as following:

```rust
extern crate bft_rs as bft;

use bft::{actuator::BftActuator as BFT, *};
```

Third, initialize a BFT actuator:

```rust
let actuator = BFT::new(address);
```

*The `address` here is the address of this node with type `Vec<u8>`.*

What needs to illustrate is that the BFT machine is in stop step by default, therefore, the first thing is send `BftMsg::Start` message. Use `send_start()` function to send a message to BFT state machine. LikeWise use `send_proposal()`, `send_vote()`, `send_feed()`, `send_status()`, `send_pause()` functions to send `Proposal`, `Vote`, `Feed`, `Status`, `Pause` messages to the BFT actuator, these functions will return a `Result`. take `Status` for example:

```rust
actuator.send_start(BftMsg::Start).expect("");

actuator.send_status(BftMsg::Status(status)).expect("");

// only in feature verify_req
actuator.send_verify(BftMsg::VerifyResq(result)).expect("");
```

And use `recv()` function and `match` to receive messages from BFT state machine as following:

```rust
if let Ok(msg) = actuator.recv() {
      match msg {
            BftMsg::Proposal(proposal) => {}
            BftMsg::Vote(vote) => {}
            BftMsg::Commit(commit) => {}
            _ => {}
      }
}
```

If you want to use the BFT height to do some verify, use `get_height` function as following:

```rust
let height: u64 = actuator.get_height();
```

## License

This an open source project under the [MIT License](https://github.com/cryptape/bft-rs/blob/master/LICENSE).
