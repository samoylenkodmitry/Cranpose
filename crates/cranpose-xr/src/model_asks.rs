//! When the backend asks the runtime for a controller's model.

use crate::{Hand, InteractionProfile, controller_look::Offer};

// How far the questions of one hand have come.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    // The runtime has not answered yet
    Untried,
    // The runtime had no model that it could give. Another controller in the hand makes it ask again.
    Waiting,
    // The runtime gave its model
    Done,
}

#[derive(Debug)]
pub(crate) struct ModelAsks {
    steps: [Step; 2],
    // Whether each hand held a controller when the schedule last looked
    held: [bool; 2],
}

impl ModelAsks {
    pub(crate) fn new() -> Self {
        Self {
            steps: [Step::Untried; 2],
            held: [false; 2],
        }
    }

    // The hands to ask now, given the interaction profile of each hand as the last frame reported
    // it. A hand that the runtime has not answered is due. A hand whose answer was no model is due
    // again when a controller comes into it, which a hand profile never does. A hand that got its
    // model is never due. A call with nothing due touches no runtime.
    pub(crate) fn due(&mut self, profiles: [Option<InteractionProfile>; 2]) -> [bool; 2] {
        let mut due = [false; 2];
        for hand in Hand::BOTH {
            let index = hand.index();
            let held = profiles[index].is_some_and(|profile| !profile.is_hand());
            due[index] = match self.steps[index] {
                Step::Untried => true,
                Step::Waiting => held && !self.held[index],
                Step::Done => false,
            };
            self.held[index] = held;
        }
        due
    }

    // Records what the runtime answered for `hand`: its model, or none.
    pub(crate) fn answered(&mut self, hand: Hand, offer: &Offer) {
        self.steps[hand.index()] = match offer {
            Offer::Bytes(_) => Step::Done,
            Offer::Pending | Offer::Absent => Step::Waiting,
        };
    }
}

#[cfg(test)]
#[path = "tests/model_asks_tests.rs"]
mod tests;
