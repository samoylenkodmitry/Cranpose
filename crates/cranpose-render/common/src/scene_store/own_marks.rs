// What each own draw of a record draws and where, as an update last left
// them: the next update that redraws the record finds which of them changed.
// While no reader follows where records redraw, an update that redraws a
// record loses the marks; a reader that follows that again, or asks for
// reach, finds them worked out anew for every record.

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
        // A reader that stopped following where records redraw since the
        // record waited for this end of the update.
        if !follow {
            lose(store, index);
            continue;
        }
        store.tree[index].flags =
            flags.without(RecordFlags::REDRAW_PENDING.with(RecordFlags::INSERTED));
        // With the marks lost, a record does not know what it drew before,
        // and the refresh below marks it. A record made in the update is new
        // as a whole.
        let changed = if store.marks_lost {
            Reach::Unknown
        } else {
            remark(store, id)
        };
        let changed = if flags.contains(RecordFlags::INSERTED) {
            Reach::Empty
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
        refresh_lost(store);
    }
}

// Loses the marks for the redraw of the record at `index` while no reader
// follows where records redraw, and logs the redraw without its reach. A
// record the update made logs none.
#[inline(always)]
pub(super) fn lose(store: &mut SceneStore, index: usize) {
    let flags = store.tree[index].flags;
    store.tree[index].flags =
        flags.without(RecordFlags::REDRAW_PENDING.with(RecordFlags::INSERTED));
    store.marks_lost = true;
    if !flags.contains(RecordFlags::INSERTED) {
        store.log(
            RecordEvent::Redrawn(Reach::Unknown),
            RecordId::from_index(index).expect("a row is a record"),
        );
    }
}

// Works out the marks of every record anew once they were lost.
pub(super) fn refresh_lost(store: &mut SceneStore) {
    if !std::mem::take(&mut store.marks_lost) {
        return;
    }
    if let Some(root) = store.root() {
        remark_subtree(store, root);
    }
}

fn remark_subtree(store: &mut SceneStore, id: RecordId) {
    remark(store, id);
    let span = store.tree[id.index()].items;
    for position in 0..span.len() {
        if let Some(child) = store.items.span(span)[position].as_child() {
            remark_subtree(store, child);
        }
    }
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
