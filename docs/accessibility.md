# Accessibility

What a Cranpose app owes a person who cannot see the screen, and what the
framework already does for it. The API mirrors Jetpack Compose, so a Compose
developer writes the same three things: semantics on a control, focus movement,
and text to read out.

Four platforms carry it: iOS through VoiceOver, Android through TalkBack, the
web through a DOM mirror that a browser screen reader reads, and Linux, macOS
and Windows through accesskit (Orca, VoiceOver, NVDA, Narrator).

## 1. Name every control

```rust
Modifier::empty().semantics(|config| {
    config.content_description = Some("Import receipts".into());
    config.role = Some(SemanticsWidgetRole::Button);
    config.state_description = Some("7 waiting".into());
})
```

`SemanticsConfiguration` mirrors Compose field for field:
`content_description`, `state_description`, `on_click_label`, `role`,
`selected`, `toggled`, `enabled`, `custom_actions`, `is_modal`. A `Text`
carries its own string, so a label is needed only where the text on screen is
not the whole story.

A control drawn on a canvas has no layout node of its own. `canvas_children`
gives each drawn control its own bounds, label, role and actions, so a reader
reaches a ring segment the same way it reaches a button.

A debug build says so when a control takes a click or text and has no name:
the log carries one `accessibility: control <id> takes a click or text but has
no label` line per node. A release build stays quiet.

## 2. Focus

```rust
Modifier::empty().focusable()

let focus = cranpose_ui::local_focus_manager().current();
focus.move_focus(FocusDirection::Next);
focus.clear_focus();
```

Tab and Shift+Tab move focus with no app code: the app shell asks the focus
manager, which walks the focus targets in layout order. `FocusDirection::Up`,
`Down`, `Left` and `Right` pick the nearest target in that direction, measured
on the laid-out rectangles.

Focus travels both ways across every platform boundary:

* the semantics tree reports `focusable` and `focused` on each node;
* a screen reader that lands its cursor on a control moves the app's focus
  (`accessibilityElementDidBecomeFocused`, TalkBack's focus event, a `focusin`
  on the web mirror, accesskit's `Action::Focus`);
* a focus move the app made moves the reader's cursor
  (`UIAccessibilityLayoutChangedNotification`, `AccessibilityNodeInfo` focus,
  `element.focus()`, the accesskit tree focus).

## 3. Text with no control behind it

Two ways, the way Compose has two.

A control whose text changes on its own is a live region:

```rust
Modifier::empty().semantics(|config| {
    config.live_region = Some(LiveRegionMode::Polite);
})
```

`Polite` waits for the reader to finish its sentence; `Assertive` cuts in. A
live region on a container reaches every control under it.

An event with no control behind it goes through the announcer:

```rust
let reader = cranpose_ui::local_announcer().current();
reader.announce("Seven receipts imported");
reader.announce_assertive("Import failed");
```

Both reach the same four platforms:

| Platform | Live region | Announcement |
| --- | --- | --- |
| accesskit (Linux, macOS, Windows) | `Live::Polite` / `Live::Assertive` on the node | a live node under the window, whose value carries the text |
| iOS | no such notion: Cranpose reads the changed text out | `UIAccessibilityAnnouncementNotification` |
| Android | TalkBack reads a virtual view's live region only through its host view, so Cranpose reads the changed text out | `announceForAccessibility` on the host view |
| Web | the DOM mirror is rebuilt on every change, and a live region that appears together with its text is read by no reader, so Cranpose reads the changed text out | a hidden `aria-live` region that outlives the mirror |

Where Cranpose reads the text out itself, it compares the new semantics
snapshot against the one before it. A live region that just appeared is read;
the first snapshot of a screen is not, or a blind user would hear the whole
screen twice.

The announcer holds at most 32 lines. An app that announces in a loop loses the
oldest lines rather than growing without limit.

## 4. A value inside a range

A slider, a dial or a progress bar reads as plain text unless the platform
knows it holds a value. A person who cannot see the screen then hears "47
percent" and has no way to change it.

```rust
Modifier::empty().progress_semantics(value, 0.0, 1.0, 0)

Modifier::empty().semantics(|config| {
    config.progress = Some(ProgressBarRangeInfo::new(value, 0.0, 1.0, 0));
    config.set_progress = Some(SemanticsSetProgress::new(move |next| {
        on_value_change(next);
        true
    }));
})
```

`progress_semantics` is Compose's `Modifier.progressSemantics(value, range,
steps)`; `set_progress` is `SemanticsActions.SetProgress`. The built-in
`Slider` declares both, so an app that uses it gets an adjustable control with
no further code. `steps` counts the stops between the two ends; zero means the
value moves freely, and a reader's step is then a tenth of the range.

| Platform | Reads | Moves |
| --- | --- | --- |
| accesskit | `Role::Slider` with the value, its ends and its step | `Action::SetValue`, `Increment`, `Decrement` |
| iOS | `UIAccessibilityTraitAdjustable` with the state text as the value | a VoiceOver swipe up or down, through `accessibilityIncrement` and `accessibilityDecrement` |
| Android | `RangeInfo` on the node | TalkBack's `ACTION_SET_PROGRESS`, or a volume key swipe |
| Web | `role="slider"` with `aria-valuenow`, `min`, `max`, `valuetext` | the arrow keys, Home and End on the mirrored control |

The value goes back through `set_progress` on the live semantics tree, so a
stale published snapshot cannot move the wrong control.

## 4b. An action a reader picks by name

A row that dismisses on a swipe, a text with three links: a person who cannot
see the screen cannot make the swipe or aim at one word. `config.custom_actions`
is Compose's `customActions`; each entry has a label and a handler, and every
platform lists it under the control.

| Platform | Where it shows | How it runs |
| --- | --- | --- |
| accesskit | the node's custom actions | `Action::CustomAction` with the index |
| iOS | VoiceOver's actions rotor, as `UIAccessibilityCustomAction` | `performAccessibilityCustomAction:` on the element, matched by name |
| Android | the node's actions, as TalkBack's actions menu | `nativeOnAccessibilityCustomAction` with the index |
| Web | one `<button>` per action right after the control, "Dismiss, Milk" | a click on that button |

`SwipeToDismiss` and `LinkedText` fill this on their own.

## 5. A list a reader can page

A lazy list builds only the rows on screen. A reader that walks the rows one
by one reaches the last built row and stops; the rows below it do not exist
yet. The list has to tell the platform that it scrolls, and take a page move
back.

```rust
Modifier::empty().semantics(|config| {
    config.vertical_scroll = Some(ScrollAxisRange::new(offset, max_offset, false));
    config.scroll_by = Some(SemanticsScrollBy::new(move |dx, dy| state.scroll_by(dx, dy)));
})
```

A lazy list also says how many rows it holds, through `config.collection`;
`LazyColumn` and `LazyRow` do this on their own. TalkBack reads "list, 12
items" as its cursor enters the list, and accesskit gives the container the
list role. VoiceOver and the web mirror have no place for the count.

`vertical_scroll` and `horizontal_scroll` are Compose's
`verticalScrollAxisRange` and `horizontalScrollAxisRange`; `scroll_by` is
`SemanticsActions.ScrollBy`. `verticalScroll`, `horizontalScroll`,
`LazyColumn` and `LazyRow` declare both on their own. A plain scroll reports
its offset in pixels; a lazy list reports the index of its first visible row,
because it does not know the height of the rows it has not built.

The container itself has no name, so a reader's cursor does not stop on it.
One page is nine tenths of what the container shows, so the last row of one
page is still on the next. The move goes to the container above the element
under the reader's cursor: each element names the scrollable node closest
above it in the semantics tree, so a row one page has moved off screen still
pages its own list. The move runs through `scroll_by` on the live tree.

| Platform | Reads | Pages |
| --- | --- | --- |
| accesskit | `Role::ScrollView` with the offset and its range, and the rows as its children | `Action::ScrollDown`, `ScrollUp`, `ScrollRight`, `ScrollLeft` |
| iOS | nothing; the container stays out of the cursor's way | a VoiceOver three-finger swipe, through `accessibilityScroll:` on the focused element |
| Android | `isScrollable` with `ACTION_SCROLL_FORWARD` and `BACKWARD` as the offset allows; each row sits under its list in the virtual view tree, and a list with no text is not focusable | TalkBack's page gesture on a row, which reaches the list above it |
| Web | nothing on the mirror | Page Down and Page Up on the focused mirrored element |

## 6. A way out of a dialog

A person who cannot see the screen opens a menu or a dialog and needs a way
back that does not depend on a "Cancel" button somewhere on it. Every reader
has one gesture for this, and Cranpose routes all of them to the same place:
the innermost open modal, the one the platform back gesture closes.

`Dialog` registers itself on the modal stack while it is open. Nothing else is
needed: an app that shows a `Dialog` gets all four ways out below. With no
dialog open, the same gesture closes the dismissable popup on top instead, a
dropdown or a menu shown through `PopupDismissable`, the way an outside tap
would. An app that handles back on its own does so through `BackHandler`, as
on Android.

A dialog that opens takes app focus, so the reader's cursor lands on it with
no app code: VoiceOver gets a screen change aimed at the dialog, TalkBack gets
the focus event the Android host sends for every app focus move, the web
mirror focuses the dialog's node, and accesskit follows the app focus it
already receives.

| Platform | Gesture | Where it goes |
| --- | --- | --- |
| accesskit | Escape on the keyboard | `AppShell::dismiss_top_modal` |
| iOS | a VoiceOver two-finger scrub, through `accessibilityPerformEscape` | the dialog on top; with none open, the app's `BackHandler` |
| Android | TalkBack's back gesture, which is the system back key | the dialog on top, then the app's `BackHandler` |
| Web | Escape on the keyboard, with focus on the mirror or the canvas | `AppShell::dismiss_top_modal` |

## 7. A control that changes under the cursor

A reader that activates a toggle, or sits on a counter while the app moves
it on, needs to hear the new state without moving its cursor. Each publish
compares every control with its last publication; one that now says
something else is marked as changed.

| Platform | What the change does |
| --- | --- |
| accesskit | The tree update carries the new value; the reader speaks it on its own. |
| iOS | A layout change names the element under the VoiceOver cursor, which reads it again. |
| Android | A content-changed event with the text, description and state change types goes out for the control, and TalkBack speaks the one under its cursor. |
| Web | The mirror node keeps its focus and takes the new label; a reader speaks it on the next move. Text that has to be heard at once is a live region. |

## 8. Where a reader is

A person who cannot see the screen needs to hear where the app took them
when it moves on by itself: a receipt opens after a scan, a folder replaces
the list. `Modifier::pane_title("Receipt")` on the root of a screen names it,
Compose's `paneTitle`. Every publish compares the titles with the last one
and reads a new or changed title out the way a live region is read: TalkBack
and VoiceOver speak it, the web mirror's live region carries it, and the
accesskit tree announces it. The root itself is not a stop; on Android it
carries the pane title of its node, on the web it is a region landmark with
the title as its name, and accesskit sees a labeled region. The first
publish stays quiet, so the first screen is not read twice.

## What the built-in widgets say on their own

An app gets this with no code of its own:

| Widget | A reader hears | A reader can |
| --- | --- | --- |
| `Text` | the text | |
| `Button`, `clickable` | the label, "button" | activate it |
| `toggleable`, a switch or checkbox | the label, its state | flip it |
| `selectable`, a tab or a radio row | the label, its role, whether it is picked | pick it |
| `LiquidTabBar` | the tab, whether it is picked, and which of how many | pick it |
| `BasicTextField` | the name the app gave it, or the text it holds; an empty field is still a stop | type into it, or hand it whole text |
| `Slider` | the value | move it |
| `CircularProgressIndicator`, `LinearProgressIndicator` | "Loading" | |
| `SwipeToDismiss` | the row's content | run "Dismiss" from the actions menu |
| `verticalScroll`, `horizontalScroll`, `LazyColumn`, `LazyRow` | the rows inside, and on Android how many rows there are | page on and back |
| `LinkedText` | the whole text | open each link from the actions menu, as "Open <link text>" |
| `Dialog` | its content, and nothing outside it; the reader lands on it as it opens | leave it with the reader's escape gesture |
| `Image`, `Icon` | the description the app gave | |

A control an app draws itself declares what it is through
`Modifier::semantics`; see section 1.

A text field takes its name from `Modifier::content_description` on the
field, and the text it holds is its value: a reader hears "Folder name, text
field, Milk". With no name the text stands in for it, and a field that is
empty as well is still a stop that says "text field", so a reader can find
it and type. A debug build logs a warning for such a field.

A reader or a voice tool can also hand a field whole text at once: the
set-text action on Android, which TalkBack's braille keyboard and Voice
Access use, and accesskit's set-value action on the desktop. VoiceOver and
the web type through the keyboard.

The other way round, `Modifier::hide_from_accessibility()` takes a node and
everything under it out of what a reader sees: a decorative image, or a
placeholder drawn under a field that already carries the same words as its
name.

A row of texts that belong together, a name with its count and its price,
reads as three stops unless the app says otherwise. `Modifier::merge_descendants()`
on the row makes it one stop, "Milk, 2, 3.40", the way a button with text
inside already reads; the texts under it are not published on their own.

A row of tabs or radio buttons is a group, and a reader says which of how
many its cursor is on. `Modifier::selectable_group()` on the row declares it;
`LiquidTabBar` does so on its own. TalkBack says "Library, tab, 2 of 5",
VoiceOver reads "2 of 5" as the tab's value, the web mirror sets
`aria-posinset` and `aria-setsize`, and accesskit gets the position and the
size of the set.

## What a reader hears, end to end

1. Layout builds the semantics tree, one node per control, with focus flags.
2. `crates/cranpose/src/accessibility.rs` projects that tree onto flat elements
   with screen bounds: one platform-neutral shape, four bridges.
3. Each bridge turns an element into the platform's own node, and turns the
   platform's actions back into a click, a custom action, a value, a page or
   a focus move.
4. An action that reaches the live tree runs through
   `accessibility::run_reader_action`, inside the app context and a mutable
   snapshot, the way a click or a key runs. Outside that context the first
   state write trips the render state; the static test
   `every_reader_action_runs_inside_the_app_context` keeps every bridge on it.

Nothing in that path is platform specific above the bridge, so a control that
reads correctly on one platform reads correctly on all four. The static test
`every_platform_bridge_reads_announcements_out` and its focus counterpart in
`crates/cranpose/tests/platform_scheduling_static.rs` keep the four bridges in
step.

## Check the web mirror without a hand

`scripts/a11y/web-page-check.mjs <url>` drives a headless Chrome over the
DevTools protocol against a served web demo (`apps/desktop-demo/build-web.sh
--release`, then `package-web.sh` and any static server): it focuses a button
in the tab row, presses Page Down and Page Up, reads the mirror's positions
and the browser console, and prints one JSON report. A page that works moves
the row by nine tenths of its width and back, keeps the focus on the same
button, and leaves no panic in the console. The report also lists the text
fields of the Text Input page: an empty one is a stop with an empty label,
and each of the others carries its text as content.

## Check it by hand

* iOS: Settings, Accessibility, VoiceOver. Swipe right to walk the controls.
* Android: Settings, Accessibility, TalkBack.
* Web: the page carries a mirror of every control; NVDA, VoiceOver or Orca
  read it.
* Linux: Orca, with `accessibility` enabled in the desktop settings.
