# BFT

An efficent and stable Rust library of BFT protocol for distributed system.

## What is BFT?

BFT(Byzantine Fault Tolerance) comprise a class of consensus algorithms that achieve byzantine fault tolerance. BFT can guarantee liveness and safety for a distributed system where there are not more than 33% malicious byzantine nodes, and thus BFT is often used in blockchain network.

## BFT Protocol

### Protocol

BFT is an State Machine Replication algorithm, and some states are shown below:

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

1. Consensus Module, the consensus algorithm module includes signature verification, proof generation, version check, etc;

2. State Machine, the BFT state machine is focused on consensus proposal;

3. Transport Module, the network for consensus module to communicate with other modules;

4. Wal Module, the place saving BFT logs.

**NOTICE**: The bft-rs only provides a basic BFT state machine and does not support the advanced functions such as signature verification, proof generation, compact block, etc. These functions are in consensus module rather than bft-rs library.

## Interface

If bft-rs works correctly, it need to receive 4 types of message: `Proposal`, `Vote`, `Feed`, `Status`. And  bft-rs can send 3 types of message: `Proposal`, `Vote`, `Commit`. These types of messages consist the `enum BftMsg`:

```rust
enum BftMsg {
    Proposal(Proposal),
    Vote(Vote),
    Feed(Feed),
    Status(Status),
    Commit(Commit),
}
```

For detailed introduction, click [here](src/lib.rs).

## Usage

First, add bft-rs and crossbeam to your `Cargo.toml`:

```rust
[dependencies]
bft-rs = { git = "https://github.com/cryptape/bft-rs.git", branch = "develop" }
crossbeam = "0.7"
```

Second, add BFT and channel to your crate as following:

```rust
extern crate bft_rs as bft;
extern crate crossbeam;

use bft::algorithm::BFT;
use bft::*;
use crossbeam::crossbeam_channel::unbounded;
```

Third, start a BFT state machine:

```rust
let (main_to_bft, bft_from_main) = unbounded();
let (bft_to_main, main_from_bft) = unbounded();

BFT::start(bft_to_mian, bft_from_main, address);
```

*The `address` here is the address of this node with type `Vec<u8>`.*

Use `send()` function to send a message to BFT state machine, take `Status` for example:

```rust
main_to_bft.
      send(BftMsg::Status(Status {
            height: INIT_HEIGHT,
            interval: None,
            authority_list: AUTH_LIST,
      }))
      .unwrap();
```

And use `recv()` function and `match` to receive messages from BFT state machine as following:

```rust
if let Ok(msg) = main_from_bft.recv() {
      match msg {
            BftMsg::Proposal(proposal) => {}
            BftMsg::Vote(vote) => {}
            BftMsg::Commit(commit) => {}
            _ => {}
      }
}
```

## License

This project is licensed under the terms of the [MIT License](https://github.com/cryptape/bft-rs/blob/master/LICENSE).
