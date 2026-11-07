use super::*;

// The visits of a walk that takes, in each room, the door `pick` names.
fn walk(steps: usize, pick: impl Fn(usize, &Plan) -> usize) -> Vec<Visit> {
    let mut visits = vec![Visit::START];
    for step in 0..steps {
        let visit = visits[visits.len() - 1];
        let door = pick(step, &Plan::of(visit.room));
        visits.push(visit.through(door));
    }
    visits
}

#[test]
fn the_door_the_eye_came_in_by_leads_back_to_the_room_it_left() {
    let visits = walk(40, |step, plan| 1 + step % (plan.doors.len() - 1));
    for pair in visits.windows(2) {
        let (before, after) = (pair[0], pair[1]);
        let back = after.back.expect("a room walked into has a way back");
        let returned = after.through(back.door);
        assert_eq!(returned.room, before.room);
        assert_eq!(
            returned.beyond(back.its_door).0,
            after.room,
            "and the door of the room left leads on to the same room again"
        );
    }
}

#[test]
fn every_room_has_a_way_on_and_its_doors_do_not_overlap() {
    for visit in walk(300, |step, plan| step % plan.doors.len()) {
        let plan = Plan::of(visit.room);
        assert!(plan.doors.len() >= 2, "{:?} has no way on", visit.room);
        for (index, door) in plan.doors.iter().enumerate() {
            assert!(
                !plan.doors[index + 1..].contains(door),
                "{:?} opens two doors at {door:?}",
                visit.room
            );
        }
    }
}

#[test]
fn walls_and_doorways_close_every_side_of_a_room() {
    for visit in walk(300, |step, plan| step % plan.doors.len()) {
        let plan = Plan::of(visit.room);
        let (width, depth) = plan.size();
        for side in Side::ALL {
            let length = if matches!(side, Side::South | Side::North) {
                width
            } else {
                depth
            };
            let mut spans: Vec<(f32, f32)> = plan
                .panels()
                .into_iter()
                .filter(|panel| panel.side == side && panel.bottom == 0.0)
                .map(|panel| (panel.from, panel.to))
                .chain(
                    plan.doors
                        .iter()
                        .filter(|door| door.side == side)
                        .map(|door| {
                            (
                                door.along() - DOOR_WIDTH * 0.5,
                                door.along() + DOOR_WIDTH * 0.5,
                            )
                        }),
                )
                .collect();
            spans.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut reached = 0.0;
            for (from, to) in spans {
                assert!(
                    (from - reached).abs() < 0.01,
                    "{:?}'s {side:?} side has a gap at {reached}",
                    visit.room
                );
                reached = to;
            }
            assert!((reached - length).abs() < 0.01);
        }
    }
}
