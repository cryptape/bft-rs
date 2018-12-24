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

use algorithm::Step;
use ethereum_types::H256;
use voteset::{Proposal, ProposalwithProof, SignProposal};

#[derive(Clone, Debug)]
pub struct ProposalMessage {
    pub height: usize,
    pub round: usize,
    pub proposal: SignProposal,
}

#[derive(Clone, Debug)]
pub struct Message {
    pub height: usize,
    pub round: usize,
    pub step: Step,
    pub proposal: Option<H256>,
}

#[derive(Clone, Debug)]
pub struct CommitMessage {
    pub height: usize,
    pub round: usize,
    pub proposal: ProposalwithProof,
}
