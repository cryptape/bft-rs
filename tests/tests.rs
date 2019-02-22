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
extern crate bft_rs as bft;
extern crate crossbeam;
extern crate rand;

use bft::algorithm::Bft;
use bft::*;
use crossbeam::crossbeam_channel::{unbounded, Receiver, Sender};

pub fn start_process(address: Address) -> (Sender<BftMsg>, Receiver<BftMsg>) {
    let (main2bft, bft4main) = unbounded();
    let (bft2main, main4bft) = unbounded();
    Bft::start(bft2main, bft4main, address);
    (main2bft, main4bft)
}

mod integration_cases;
