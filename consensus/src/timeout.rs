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

use bft_rs::bft::{Bft, Step};
use std::{thread, time};

pub fn bft_recv_timeout(engine: Bft) -> bool {
    match engine.step {
        Step::ProposeWait => {
            while engine.timer_notity.try_recv().is_err() {
                if let Ok(rp) = receive_proposal() {
                    return true;
                }
                thread::sleep(time::Duration::from_micros(50));
            }
            false
        }
        Step::Prevote => {
            while engine.timer_notity.try_recv().is_err() {
                if receive_message().is_ok() {
                    // might be panic here
                    let num = receive_message()
                        .unwrap()
                        .get_voteset(engine.height, engine.round, Step::Prevote)
                        .unwrap()
                        .count;
                    if engine.above_threshold(num) {
                        return true;
                    }
                }
                thread::sleep(time::Duration::from_micros(50));
            }
            false
        }
        Step::PrevoteWait => {
            while engine.timer_notity.try_recv().is_err() {
                if receive_message().is_ok() {
                    let num = receive_message()
                        .unwrap()
                        .get_voteset(engine.height, engine.round, Step::Prevote)
                        .unwrap()
                        .count;
                    if engine.all_vote(num) {
                        return true;
                    }
                }
                thread::sleep(time::Duration::from_micros(50));
            }
            true
        }
        Step::Precommit => {
            while engine.timer_notity.try_recv().is_err() {
                if receive_message().is_ok() {
                    // might be panic here
                    let num = receive_message()
                        .unwrap()
                        .get_voteset(engine.height, engine.round, Step::Precommit)
                        .unwrap()
                        .count;
                    if engine.above_threshold(num) {
                        return true;
                    }
                }
                thread::sleep(time::Duration::from_micros(50));
            }
            false
        }
        Step::PrecommitWait => {
            while engine.timer_notity.try_recv().is_err() {
                if receive_message().is_ok() {
                    let num = receive_message()
                        .unwrap()
                        .get_voteset(engine.height, engine.round, Step::Precommit)
                        .unwrap()
                        .count;
                    if engine.all_vote(num) {
                        return true;
                    }
                }
                thread::sleep(time::Duration::from_micros(50));
            }
            true
        }
        Step::Commit => {
            engine.timer_notity.recv();
            true
        }
    }
}