// CITA
// Copyright 2016-2017 Cryptape Technologies LLC.

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

use bft::Bft;
use timer::WaitTimer;
use params::{BftParams, Config, PrivateKey};

use std::sync::mpsc::channel;
use std::thread;

fn main() {
    let (main_to_timer, timer_from_main) = channel();
    let (timer_to_main, main_from_timer) = channel();
    let timethd = thread::spawn(move || {
        let wt = WaitTimer::new(timer_to_main, timer_from_main);
        wt.start();
    });

    let pk = PrivateKey::new(pk_path);
    let params = BftParams::new(&pk);
    
}