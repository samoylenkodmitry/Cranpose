// What each own draw of a record draws and where, as an update last left
// them: the next update that redraws the record finds which of them changed.
// While no reader follows where records redraw, the marks of the records an
// update redraws wait until a reader follows that or asks for reach.

use cranpose_core::RecordId;

use super::{LayerRef, Reach, RecordEvent, RecordFlags, SceneStore, content_hash, reach};

// One own draw of a record: a hash of what it draws, whether a child lies
// beneath it, and where it draws in the record's space.
#[derive(Clone, Copy, Debug)]
pub(crate) struct OwnMark {
    print: u64,
    over_children: bool,
    reach: Reach,
}

// Logs where the own draws of each record the update may have redrawn
// changed, and keeps their marks for the next update.
pub(super) fn settle(store: &mut SceneStore) {
    let follow = store.follows_redrawn_reach();
    let mut redrawn = std::mem::take(&mut store.redrawn);
    for record in redrawn.drain(..) {
        let Some(id) = store.resolve(record) else {
            continue;
        };
        let index = id.index();
        let flags = store.tree[index].flags;
        if !flags.contains(RecordFlags::REDRAW_PENDING) {
            continue;
        }
        let settled = flags.without(RecordFlags::REDRAW_PENDING.with(RecordFlags::INSERTED));
        if !follow {
            store.tree[index].flags = settled.with(RecordFlags::MARKS_STALE);
            if !flags.contains(RecordFlags::MARKS_STALE) {
                store.marks_stale.push(record);
            }
            if !flags.contains(RecordFlags::INSERTED) {
                store.log(RecordEvent::Redrawn(Reach::Unknown), id);
            }
            continue;
        }
        store.tree[index].flags = settled.without(RecordFlags::MARKS_STALE);
        let changed = remark(store, id);
        // A record made in the update is new as a whole; one whose marks
        // waited does not know what it drew before.
        let changed = if flags.contains(RecordFlags::INSERTED) {
            Reach::Empty
        } else if flags.contains(RecordFlags::MARKS_STALE) {
            Reach::Unknown
        } else {
            changed
        };
        if changed != Reach::Empty {
            let clip = LayerRef::new(store, id).visual_clip_rect();
            store.log(RecordEvent::Redrawn(changed.within(clip)), id);
        }
    }
    store.redrawn = redrawn;
    if follow {
        refresh_stale(store);
    }
}

// Works out the marks of the records whose marks waited.
pub(super) fn refresh_stale(store: &mut SceneStore) {
    let mut stale = std::mem::take(&mut store.marks_stale);
    for record in stale.drain(..) {
        let Some(id) = store.resolve(record) else {
            continue;
        };
        let flags = store.tree[id.index()].flags;
        if flags.contains(RecordFlags::MARKS_STALE) {
            store.tree[id.index()].flags = flags.without(RecordFlags::MARKS_STALE);
            remark(store, id);
        }
    }
    store.marks_stale = stale;
}

// Works out the marks of the own draws of `id` anew; returns where they
// differ from the marks before.
fn remark(store: &mut SceneStore, id: RecordId) -> Reach {
    let index = id.index();
    let mut marks = std::mem::take(&mut store.mark_scratch);
    marks.clear();
    let mut over_children = false;
    for item in LayerRef::new(store, id).items() {
        match (
            content_hash::own_draw_print(item),
            reach::own_draw_reach(item),
        ) {
            (Some(print), Some(reach)) => marks.push(OwnMark {
                print,
                over_children,
                reach,
            }),
            _ => over_children = true,
        }
    }
    let span = store.warm[index].own_marks;
    let changed = changed(store.marks.span(span), &marks);
    store.warm[index].own_marks = store.marks.write(span, &marks);
    store.mark_scratch = marks;
    changed
}

// Where the draws of `old` and `new` differ: those past the run they start
// with alike and before the run they end with alike.
fn changed(old: &[OwnMark], new: &[OwnMark]) -> Reach {
    let alike = |(old, new): (&OwnMark, &OwnMark)| {
        (old.print, old.over_children) == (new.print, new.over_children)
    };
    let start = old.iter().zip(new).take_while(|&pair| alike(pair)).count();
    let (old, new) = (&old[start..], &new[start..]);
    let end = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take_while(|&pair| alike(pair))
        .count();
    reach(&old[..old.len() - end]).union(reach(&new[..new.len() - end]))
}

// Where the draws of `marks` reach.
pub(super) fn reach(marks: &[OwnMark]) -> Reach {
    marks
        .iter()
        .fold(Reach::Empty, |reach, mark| reach.union(mark.reach))
}
