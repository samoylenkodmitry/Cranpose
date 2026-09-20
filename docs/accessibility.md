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
`content_description`, `state_description`, `on_click_label`, `on_long_click`,
`role`, `selected`, `toggled`, `enabled`, `custom_actions`, `is_modal`. A `Text`
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

## 4b2. A long press a reader can ask for

A row a finger opens a menu on, a photo a finger removes: a person who
cannot see the screen holds a finger on nothing and gets nothing.
`Modifier::on_long_click("Remove receipt", || …)` is Compose's
`onLongClick(label) { … }`, which `Modifier.combinedClickable` fills in for a
control that takes a long press from a finger too.

```rust
Modifier::empty()
    .content_description("Milk, 3.40")
    .on_long_click("Remove receipt", move || {
        receipts.remove(id);
        true
    })
```

The handler answers whether the control took the ask, as Compose's does. The
label is asked for rather than optional: Android is the one platform of the
four with a long press of its own, and the other three list the action by
name, so a nameless one reads as nothing. A control that declares the action
with no label through the closure form reads as "long press".

| Platform | What the reader gets |
| --- | --- |
| accesskit | one more action on the node, named by the label, after the custom actions; accesskit's `Action` enum has no long press |
| iOS | one more action in VoiceOver's actions rotor, named by the label; VoiceOver has no long press |
| Android | `ACTION_LONG_CLICK` with the label, which TalkBack reads as "double tap and hold to remove receipt" |
| Web | one more `<button>` after the control, "Remove receipt, Milk"; ARIA has no long press |

Only Android carries this as the platform's own long press. The other three
carry it as the last of the actions a reader lists, which is the same road
`custom_actions` takes.

## 4c. A field whose content is wrong

A form that shows a red line under a field tells a sighted person the
amount is not a number; a reader hears nothing unless the app says so.
`Modifier::error("needs a number")` on the field, Compose's `error`, makes a
reader hear "invalid, needs a number" after the field's state, and the
change is spoken when the field is under the cursor.

| Platform | What the reader gets |
| --- | --- |
| accesskit | the node marked invalid, with the reason in its description |
| iOS | "invalid" and the reason as the element's value, after its state |
| Android | `setContentInvalid` and `setError` on the node, which TalkBack reads as "invalid" and the reason |
| Web | `aria-invalid` on the mirror node, with the reason in `aria-description` |

## 4d. A field that holds a secret

A password field shows dots on the screen, but a screen reader asks the app
for the text, so without a mark it reads the password out loud in a room
full of people. `Modifier::password()` on the field, Compose's `password`,
keeps the text out of the projection, so it reaches no platform at all. The
field keeps the name the app gave it; with no name a reader hears
"password".

| Platform | What the reader gets |
| --- | --- |
| accesskit | the node as a password input, with no value |
| iOS | the name and "password" as the value, never the text |
| Android | `setPassword` on the node, which TalkBack reads as "password", and an empty value |
| Web | `aria-roledescription="password"` on the mirror node, with no text content |

## 4e. The order a reader walks a screen

A screen reader walks the controls in the order the app laid them out. A
search field drawn last so it sits above the rest is reached last, after
everything it filters. `Modifier::traversal_index(-1.0)` on it, Compose's
`traversalIndex`, moves it to the front of the nodes beside it; a larger
number moves a node back. Nodes left alone keep the laid-out order, so one
number on one node is the whole change.

The order is the projection's order, and every platform reads it from
there: the element list on iOS, the virtual node ids on Android, the mirror
nodes in the page on the web, and the children of the accesskit tree.

## 4f. A control a reader can open and close

An accordion row or a dropdown opens when it is pressed, but a reader has
no way to know it opens at all, and no word for whether it is open now.
`Modifier::expand(..)` says what the control does when a reader asks it to
open, and marks it closed; `Modifier::collapse(..)` says what closing it
does, and marks it open. An app swaps the two with its own state, the way
Compose's `expand` and `collapse` actions work.

| Platform | What the reader gets |
| --- | --- |
| accesskit | the node marked expanded or not, with an expand or a collapse action |
| iOS | "expanded" or "collapsed" as the element's value |
| Android | `ACTION_EXPAND` or `ACTION_COLLAPSE` on the node, which TalkBack offers in its menu |
| Web | `aria-expanded` on the mirror node |

## 4g. A dropdown and a value picker

A control that opens a list of choices read as a plain button, and a
control that steps through an ordered set read as a button too, so a
reader could not tell either from a save button. `Modifier::dropdown_list()`
and `Modifier::value_picker()`, Compose's `Role.DropdownList` and
`Role.ValuePicker`, name them.

| Platform | What the reader gets |
| --- | --- |
| accesskit | `ComboBox` and `SpinButton` |
| iOS | a button, and an adjustable element VoiceOver steps with a swipe |
| Android | `android.widget.Spinner` and `android.widget.NumberPicker` as the node's class |
| Web | `role="combobox"` and `role="spinbutton"` |

A dropdown that also declares `Modifier::expand(..)` or
`Modifier::collapse(..)` tells a reader whether the list is open.

## 4ga. A control a reader can send away

A row a sighted person swipes off, a sheet a sighted person taps outside
of: a person who cannot see the screen can make neither gesture, and the
row stays. `Modifier::dismiss(..)` says what the control does when a reader
sends it away, the way Compose's `dismiss` action does. The handler answers
whether the control took the ask.

| Platform | What the reader gets |
| --- | --- |
| accesskit | "Dismiss" among the node's actions, after the ones the app named |
| iOS | the VoiceOver two-finger scrub on the control itself |
| Android | `ACTION_DISMISS` on the node, which TalkBack lists as "Dismiss" |
| Web | one more `<button>` right after the control, "Dismiss, Milk" |

accesskit 0.24 has no dismiss action, and ARIA has none either, so on those
two the way out sits in the same list as the actions of section 4b, after
the ones the app named. On iOS a scrub on a control that declares a way out
runs that one; a scrub on any other control closes the dialog on top, as
section 6 says.

`SwipeToDismiss` declares it on its own, so a row inside one needs no app
code at all.

## 4h. One spec value, or the closure Compose takes

Compose declares semantics through a receiver lambda:
`Modifier.semantics { contentDescription = "Save" }`. Kotlin makes that
read well. Rust has no receiver lambda, so the same call is
`Modifier::semantics(|config| config.content_description = Some("Save".into()))`.
Cranpose keeps that form for parity and adds a spec value beside it:

```rust
Modifier::empty().semantics_spec(
    SemanticsSpec::new()
        .content_description("Amount")
        .error("needs a number"),
)
```

`SemanticsSpec` is `SemanticsConfiguration` under a short name, so there is
one type and one set of field names. Every builder method is named after
the field it sets. Three things the spec form gives that the closure does
not: it reads as one value rather than a body of assignments, it is one
element on the modifier chain rather than one per property, and two specs
can be compared, because the data half of the config derives `PartialEq`.

The `Modifier::` shorthands (`content_description`, `role`, `heading`,
`error`, `password`, `pane_title`, `traversal_index`, `merge_descendants`,
`selectable_group`, `hide_from_accessibility`, `live_region`,
`dropdown_list`, `value_picker`, `expand`, `collapse`) stay. They are
Cranpose's own shape, not Compose's: Compose has only `semantics`,
`clearAndSetSemantics`, `testTag`, `progressSemantics` and
`selectableGroup` as modifiers, and puts everything else inside the
lambda. One shorthand on a chain reads better than a whole spec; four or
more of them read worse, and each one costs its own chain element, so a
node with several is the place to reach for the spec.

## 4j. A field a reader moves through

A blind person who types into a field needs more than the text read back as
one piece: the caret has to move by character, by word and by line, a
stretch of text has to be picked and cut, and every move has to be heard.
The field publishes where its caret is and takes a new selection from a
reader, Compose's `setSelection`. `BasicTextField` declares both on its own,
so an app gets this with no code.

```rust
Modifier::empty().semantics(|config| {
    config.text = Some(text.clone());
    config.text_selection = Some(selection);
    config.set_selection = Some(SemanticsSetSelection::new(move |anchor, focus| {
        state.set_selection(TextRange::new(anchor, focus));
        true
    }));
})
```

The two ends are byte offsets into the text, the anchor first; equal ends
are a caret. A field that holds a secret publishes no caret.

| Platform | Reads | Moves |
| --- | --- | --- |
| accesskit | the text as text runs, one per line, with the length of each character and where each word starts, and the caret as a text selection on them | `Action::SetTextSelection` from NVDA, Narrator, VoiceOver on macOS or Orca, in characters of a run |
| iOS | the focused field is the keyboard's own `UITextInput` view, listed among the elements in the field's place with its name, hint and frame, so VoiceOver reads and moves through `UITextInput` | the rotor's characters, words and lines, and a text selection, through `setSelectedTextRange:`; a caret move the app made reaches VoiceOver through the input delegate |
| Android | `setTextSelection` on the node, a text-changed and a selection-changed event on the focused field, and `setMovementGranularities` for characters, words, lines and paragraphs on every node with text | `ACTION_NEXT_AT_MOVEMENT_GRANULARITY`, `ACTION_PREVIOUS_AT_MOVEMENT_GRANULARITY` and `ACTION_SET_SELECTION`; the host walks the text itself, sends the traversed event TalkBack speaks from, and hands the field the new caret |
| Web | the mirror node is an `<input>` or a `<textarea>` with the text as its value and the caret as its selection | the arrow keys and a reader's own text commands change the input's selection, and a `selectionchange` hands the app the new ends |

On the web a keystroke or a caret move patches the focused input in place. A
rebuild of the mirror would drop the browser's focus and make a reader hear
the whole field again instead of one character.

**The target of a decorated field.** A decorated field's semantics and its
touch target sit on the field itself, not on the box the decoration draws
around it. A field whose padding lives on the decoration box measures as
tall as its one line of text, 18 points, and fails the 24 point rule of the
audit. Put the padding on the field's modifier instead, the way the liquid
search field does with `Modifier::empty().padding_symmetric(0.0, 9.0)`, so
the padded box is what a reader and a finger get.

## 4k. Roles a reader names

A reader says what a control is after its name: "Save, button". Ten roles
covered the first apps, so a link read as a button, a search field as a
plain field, a spinner as text, and a menu, a toolbar, a tab row or a list
as nothing at all. `SemanticsWidgetRole` now also has Link, SearchField,
ProgressBar, ToggleButton, Alert, Toolbar, Menu, MenuItem, TabBar, List and
ListItem, set through `Modifier::role(..)` or the spec. The built-in widgets
declare their own: the progress indicators say progress bar, `LiquidTabBar`
says tab bar, `LiquidMenu` says menu and its rows say menu item, the liquid
search field says search field, and `LazyColumn` and `LazyRow` say list.

A toolbar, a menu, a tab bar and a list are containers: a reader walks into
them, and their rows sit under them on Android and in the accesskit tree,
the way the rows of a scroll container already do. An alert is a live
region on its own: a reader speaks it as soon as it shows.

| Role | accesskit | iOS | Android | Web |
| --- | --- | --- | --- | --- |
| Link | `Link` | the link trait | "link" as the role description | `link` |
| SearchField | `SearchInput`, with the text runs of a field | the search field trait, edited like a field | `EditText` and "search field" | `<input type="search">` |
| ProgressBar | `ProgressIndicator` | the updates-frequently trait | `ProgressBar` | `progressbar` |
| ToggleButton | `Button` with the toggled state | the button trait with the state | `ToggleButton`, checkable | `button` with `aria-pressed` |
| Alert | `Alert` | spoken when it shows | "alert", spoken when it shows | `alert` |
| Toolbar | `Toolbar` | a container | `Toolbar` | `toolbar` |
| Menu, MenuItem | `Menu`, `MenuItem` | a container, button rows | "menu", "menu item" | `menu`, `menuitem` |
| TabBar | `TabList` | a container | "tab bar" | `tablist` |
| List, ListItem | `List`, `ListItem` | a container, text rows | `ListView`, "list item" | `list`, `listitem` |

## 4l. What VoiceOver and Voice Control users expect, and whether a reader is on

Four things an app on iOS is expected to have, three of which reach the
other platforms as far as they go.

**The magic tap.** A VoiceOver user makes a two finger double tap for the
main action of a screen: take the photo, play or pause, answer the call.
`Modifier::on_magic_tap("Take the photo", || …)` says what that action is;
SwiftUI's `accessibilityAction(.magicTap)`. VoiceOver runs it on the control
under its cursor, and with the cursor on any other control it runs the first
one the screen declares, so the tap works from anywhere on the screen. The
other platforms have no such gesture, so they list the action by its label
after the control's other actions, the road the long press takes.

**Voice Control names.** Voice Control shows a short name beside each control
and the user says it. It shows the name a reader hears, which for a control
named "Import receipts from the camera roll" is a mouthful.
`Modifier::input_labels(["Import", "Import receipts"])` gives it short names
to show and take instead; SwiftUI's `accessibilityInputLabels`. iOS only:
Voice Access on Android and the desktop tools take the name a reader hears.

**A language per control.** A reader speaks with one voice unless the text
says which language it is in. `Modifier::language("de")` on a control, a BCP
47 tag, makes VoiceOver, accesskit and the browser pick the voice for it;
SwiftUI's `accessibilityLanguage`, ARIA's `lang`. TalkBack has no such
setting on a node.

**Whether a reader is on.** An app that takes a photo on its own after a
short hold, or that hides its only controls behind a swipe, needs to know
when a screen reader is on and act otherwise: speak what the camera sees,
wait for a tap. `cranpose_services::local_accessibility_state()` carries
`AccessibilityState::screen_reader_on`, and a composable that reads it is
composed again when it changes.

| Platform | Says a reader is on when |
| --- | --- |
| iOS | `UIAccessibilityIsVoiceOverRunning()` answers yes, checked on every frame the bridge publishes |
| Android | an accessibility service is enabled, the same signal that turns the node provider on |
| accesskit | a reader asked for the tree, until it lets go |
| Web | never: a browser gives a page no such signal |

`ProvideAccessibilityState(state, content)` fixes the state for a preview or a
test, the way `ProvideSystemTheme` fixes the theme.

## 4m. A keyboard and a switch reach every control

Three kinds of people work without a pointer: a blind person with a hardware
keyboard on a Mac, a PC or an iPad; a switch access user whose one or two
switches press Tab and Enter; a person with a tremor who cannot hold a
pointer still on a small target. For all of them a control that only a tap
reaches does not exist.

**Every clickable control takes focus.** `Modifier::clickable`,
`toggleable`, `selectable` and every widget built on them, the `Button`, the
liquid tabs, segments, menu rows, icon groups and the toggle, register a
focus target. Tab and Shift+Tab walk them in layout order. Nothing to
declare in an app.

**Enter and Space press the focused control.** The shell sends a press and
a release at the control's centre, so the same handler runs as for a tap,
and a widget with its own gesture code needs no second path. While a text
field holds focus the two keys go to the field.

**Arrow keys inside a group.** Under `Modifier::selectable_group()`, a tab
bar, a radio group, a segmented control, and inside a menu, the arrow keys
move focus to the nearest control in that direction and never leave the
group. Outside a group the arrows stay with the content, so a list still
scrolls.

**A ring shows where focus is.** `Modifier::focusable()` is a focus target
that draws a ring, a 2 point blue line with a 1 point white line inside it,
while the keyboard made the last focus move. The next pointer press anywhere
takes the ring away, the way `:focus-visible` works in a browser. A widget
that draws its own focus look keeps `Modifier::focus_target()`.

**A target a finger hits.** `Modifier::minimum_interactive_component_size()`
keeps at least 48 by 48 points for the press, as far as the parent allows,
and puts the drawn content in the middle. The drawn size does not change: a
20 point checkbox still looks 20 points wide and takes a press 14 points to
each side of it. Compose's `minimumInteractiveComponentSize`, Material's
48 dp and above Apple's 44 pt. `IconButton` already keeps 48 points on its
own.

A press from a finger or a pen that misses every control, but lands within
the 48 points around a smaller control, reaches that control when no other
control claims the point, the way Compose's minimum touch target hit test
does. A scroll container or another parent around the control does not
claim it; a sibling under the finger does. The nearest such control takes
it. The release then counts as a click when it stays within the drag
threshold of the press, as any release does. A mouse, or a pointer of an
unknown kind, keeps its exact point.

| Key | What happens |
| --- | --- |
| Tab, Shift+Tab | focus moves to the next or the previous control |
| Enter, Space | the focused control is pressed |
| Left, Right, Up, Down | inside a selectable group or a menu, focus moves to the nearest control that way |
| Escape | the dialog, sheet, menu or popup on top closes |

## 4i. An icon says what it is, or says it is decoration

A picture with no words under it is the one thing a screen reader cannot
work out on its own. Compose asks for the answer at the call site:
`Icon(imageVector, contentDescription)` takes the name as its second
argument, and `null` means the icon is decoration a reader should skip.

Cranpose now asks the same way. `cranpose_ui::widgets::Icon` and
`cranpose_liquid::icons::Icon` both take
`content_description: Option<String>` right after the path. A name reaches
a reader as the node's name with the image role. `None` publishes nothing,
so a reader walks past it.

`None` is the right answer more often than not, and the call sites in this
repo show why: the glyph inside a search field, an icon button, a tab or a
menu row is decoration, because the control around it already carries the
name. What the argument buys is that the writer says so on purpose rather
than by forgetting.

Two faults came out of that sweep. A liquid menu row said its label and
that it was clickable, and never said it was checked; a reader heard no
difference between the picked row and the rest, because the check mark was
the only signal. The row now carries `toggled`.

The same rule is not put on `Text` or `Button`. A `Text`'s name is its
words, and a `Button`'s name is what it holds, so an argument there would
be the empty answer at every call site, and a writer who types the empty
answer often enough stops reading it. Compose asks at the same two places
and no others.

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

## 5b. A jump to a row of a long list

Paging is slow. A list of 500 rows takes about fifty pages to cross, and a
person who cannot see the screen hears every row along the way. A reader that
can name a row goes straight to it.

```rust
Modifier::empty().scroll_to_index(move |index| {
    if index >= rows.len() {
        return false;
    }
    state.scroll_to_item(index, 0.0);
    true
})
```

This is Compose's `SemanticsActions.ScrollToIndex`. `LazyColumn` and `LazyRow`
declare it on their own, beside the scroll range and the row count they
already declare, so an app written with them needs no code for this. The
index counts rows from zero and the answer says whether the list moved: a
number past the last row moves nothing. The row the reader named goes to the
start of the list.

Only Android has an action for this. The other three platforms carry what
they can, and the table says which:

| Platform | What a reader does | What it can reach |
| --- | --- | --- |
| Android | TalkBack, or any other service, sends `ACTION_SCROLL_TO_POSITION` with a row or a column argument | any row |
| accesskit | the reader reads the list's scroll range, which counts rows, and sets an offset with `SetScrollOffset` | any row |
| iOS | the VoiceOver actions rotor on any row of the list offers "first row" and "last row" | the two ends |
| Web | Home and End on a focused mirrored row | the two ends |

accesskit 0.24.1 has no scroll-to-index action and neither has ARIA, so
neither claims one. On accesskit the list carries its rows as the scroll
offset of the axis it runs along: `scroll_y` is the first visible row,
`scroll_y_max` is the last row, and an offset a reader sets is a row number.
VoiceOver and the web mirror have no way for a person to type a number at a
list at all, so the two ends are what they offer, through the custom-action
road and through the keys a list already answers. A text field moves its
caret with Home and End and a slider moves its value, so those two keep the
keys even inside a list; every other mirrored row hands them to the list.

Compose's `SemanticsActions.IndexForKey` has no twin here. It turns an app's
own item key into a row number, and no screen reader on any of the four
platforms asks for it; in Compose it serves the test finder, not a reader.

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
When the dialog closes, app focus returns to the control that had it before
the dialog opened, so a reader lands back on the button it pressed rather
than at the top of the screen.

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

## 9. The system's display options

A person sets these once, in the system's accessibility settings, and expects
every app to follow: larger text, less motion, less transparency, more
contrast, bold text, inverted colors. Cranpose reads them on every platform
and acts on them in the framework, so an app follows them with no code of
its own. `cranpose_services::AccessibilityOptions` holds them;
`local_accessibility_options().current()` reads them in a composable for
what an app draws itself, and `ProvideAccessibilityOptions` fixes them for a
preview or a test.

| Option | What the framework does | iOS | Android | Web | Desktop |
| --- | --- | --- | --- | --- | --- |
| `font_scale` | every `Sp` text size grows by it, through the shell's font scale | Dynamic Type, each step as its body size over 17 points | the font size setting, through Android's own curve | the root font size over 16 pixels, as page text does | GNOME's text scaling factor, Windows' text size, `CRANPOSE_FONT_SCALE` |
| `reduce_motion` | every animation ends on its first frame: the value is the target at once | Reduce Motion | Remove animations, the animator scale at zero | `prefers-reduced-motion` | macOS Reduce Motion, GNOME's animations switch, Windows' animation switch, `CRANPOSE_REDUCE_MOTION` |
| `reduce_transparency` | glass draws as the surface color with no blur, no refraction and no spectrum; the shape and the shadow stay | Reduce Transparency | no such setting | `prefers-reduced-transparency` | macOS Reduce Transparency, Windows' transparency switch, `CRANPOSE_REDUCE_TRANSPARENCY` |
| `increase_contrast` | secondary and tertiary labels, separators, fills and the glass edge move toward the label color | Increase Contrast | High contrast text | `prefers-contrast: more` | macOS Increase Contrast, a GNOME high contrast theme, Windows high contrast, `CRANPOSE_INCREASE_CONTRAST` |
| `bold_text` | every liquid text style gains two hundred of weight | Bold Text | Bold text, a font weight adjustment of 300 | no such setting | `CRANPOSE_BOLD_TEXT` |
| `invert_colors` | the theme swaps to its other palette and pictures stay as they are | Smart Invert: the window opts out of the system's inversion and inverts its own colors | no: the system inverts the whole screen itself | no | no |

The desktop reads its settings tools once, on a thread at start, and applies
the answer on the next frame; a change while the app runs takes a restart. A
run a robot drives reads only the environment variables, so a screenshot test
does not follow the host's text scale.
iOS, Android and the web follow a change at once. The demo's robot and a test
set an option through the environment or `ProvideAccessibilityOptions`; the
static test `every_platform_reports_the_display_options_and_the_framework_acts_on_them`
keeps the four platforms and the three framework hooks in step.

## What the built-in widgets say on their own

An app gets this with no code of its own:

| Widget | A reader hears | A reader can |
| --- | --- | --- |
| `Text` | the text | |
| `Button`, `clickable` | the label, "button" | activate it |
| `toggleable`, a switch or checkbox | the label, its state | flip it |
| `selectable`, a tab or a radio row | the label, its role, whether it is picked | pick it |
| `LiquidTabBar` | the tab, whether it is picked, and which of how many | pick it |
| `LiquidToggle`, `LiquidSegmented`, `LiquidChip`, `LiquidMenu` rows, the liquid icon group | the switch and its state, the segment or the chip and whether it is picked, the row | flip or pick it, from a reader, Tab and Enter alike |
| `BasicTextField` | the name the app gave it, or the text it holds; an empty field is still a stop | type into it, hand it whole text, move the caret by character, word and line, and pick a stretch of text |
| `Slider` | the value | move it |
| `LiquidSlider` | the name the app gave it, and the value in percent | move it with a swipe up or down |
| `CircularProgressIndicator`, `LinearProgressIndicator` | "Loading", progress bar | |
| `SwipeToDismiss` | the row's content | send the row away with the reader's own dismiss, or run "Dismiss" from the actions menu |
| `verticalScroll`, `horizontalScroll`, `LazyColumn`, `LazyRow` | the rows inside, and on Android how many rows there are | page on and back |
| `LinkedText` | the whole text | open each link from the actions menu, as "Open <link text>" |
| `Dialog` | its content, and nothing outside it; the reader lands on it as it opens | leave it with the reader's escape gesture, and land back on the control that opened it |
| `Image`, `Icon` | the description the app gave | |

A control an app draws itself declares what it is through
`Modifier::semantics`; see section 1.

A text field takes its name from `Modifier::content_description` on the
field, and the text it holds is its value: a reader hears "Folder name, text
field, Milk". With no name the text stands in for it, and a field that is
empty as well is still a stop that says "text field", so a reader can find
it and type.

A reader or a voice tool can also hand a field whole text at once: the
set-text action on Android, which TalkBack's braille keyboard and Voice
Access use, and accesskit's set-value action on the desktop. The web mirror
forwards native input events, including dictated or pasted text and the
selection, to the same field callbacks. VoiceOver uses the native keyboard.

The other way round, `Modifier::hide_from_accessibility()` takes a node and
everything under it out of what a reader sees: a decorative image, or a
placeholder drawn under a field that already carries the same words as its
name.

A row of texts that belong together, a name with its count and its price,
reads as three stops unless the app says otherwise. `Modifier::merge_descendants()`
on the row makes it one stop, "Milk, 2, 3.40", the way a button with text
inside already reads; the texts under it are not published on their own.
Independent controls remain separate: a nested button, editable field, slider,
custom-action control, or explicitly merged group contributes nothing to its
ancestor's name. Names follow traversal order and retain repeated words.
Hidden subtrees and password values never contribute text to an ancestor.
`SemanticsNode::accessibility_label()` supplies the name to both the platform
projection and the placed tree used by accessibility audits.

Reader callbacks resolve the live tree and reject disabled nodes and hidden
subtrees, including requests queued before the node changed state. Disabled
canvas actions also reject activation. Android text search matches names and
editable values within the requested virtual subtree. Determinate progress
indicators retain the progress-bar role and numeric range; a range is adjustable
only when the control provides its adjustment action.

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

The shared projection keeps names and control boundaries consistent. Each
platform still needs its own behavioral checks. The static test
`every_platform_bridge_reads_announcements_out` and its focus counterpart in
`crates/cranpose/tests/platform_scheduling_static.rs` keep the four bridges in
step.

## Check a screen without a hand

Four checks run in a test, so a screen that a reader user cannot use fails
the build and not the person.

**`assert_accessible`.** `cranpose_testing::assert_accessible(&placed)` walks
a placed semantics tree and fails the test with every issue and what fixes
it. `ComposeTestRule::assert_accessible(size)` runs it on composed content,
`RobotTestRule::assert_accessible()` on what a headless shell shows, and
`audit_accessibility` returns the list for a test that wants to look at it.
Over the external robot, `robot.audit_accessibility()` returns the same list
for the screen a running app shows, and `robot.assert_accessible()` fails on it;
`audit_changes` compares that list with the issues a suite leaves as they are.
The issues:

| Issue | What a reader user hits | The fix |
| --- | --- | --- |
| `NoName` | a control that says nothing | `content_description`, or a `Text` inside it |
| `SameName` | two controls of one role with one name, "Delete" and "Delete"; rows of a list at different places are apart, a reader speaks their place | say what each acts on |
| `SmallTarget` | a control under 24 by 24 points, WCAG 2.5.8 | `Modifier::minimum_interactive_component_size()` |
| `OutOfOrder` | a control laid out fully above the one read before it | reading order, or `traversal_index` |
| `NoPaneTitle` | a screen that says nothing on arrival | `pane_title` on the root |
| `UnnamedImage` | a picture a reader stops on with no words | a description, or `None` so it is skipped |

**The demo screens.** `apps/desktop-demo/tests/accessibility_audit.rs` puts
all 27 tabs of the desktop demo under `audit_accessibility`. The tabs that
were caught the first time are named there beside the reason each is left
as it is, and the list only shrinks: a new issue on any tab fails the test,
and so does a listed issue that went away without the list saying so.

**Android's own checks.** `CranposeAccessibilityAuditTest` in the Android
demo runs Google's Accessibility Test Framework, the checks behind the
Accessibility Scanner app, over the node tree the activity publishes: a name
on every control, no two controls with one name, a 48 dp touch target and
text contrast. It runs with the other instrumented tests on a device or an
emulator.

**The tree a reader speaks.** `robot.spoken_tree()` returns the screen the
way VoiceOver reads it, one control per line: the name, the role, the state,
the value, the actions. A robot test compares the lines; a person prints them
to look at a screen from a terminal. `docs/ROBOT_TESTING.md` has the shape.

## Check the web mirror without a hand

Run `just test-web-accessibility <url>` against a release demo built with
`just web`, packaged with `apps/desktop-demo/package-web.sh`, and served by a
static server. The test drives headless Chrome through the DevTools protocol
and exits with failure on an unmet assertion. Set `CHROME` to choose the
browser executable and `A11Y_DEBUG_PORT` to choose its debugging port.

The checks cover DOM identity and focus after a counter changes, forward and
backward Tab navigation, one activation per Enter press, valid Tab indices,
list ownership, native dictated input, backward selection, and keyboard editing.
The mirror patches retained nodes and action buttons, removes obsolete
attributes and nodes, and nests children under their semantic containers.
Keeping the DOM node alive preserves the object a reader's virtual cursor
refers to; restoring browser focus after replacing that node is insufficient.

## Check it by hand

* iOS: Settings, Accessibility, VoiceOver. Swipe right to walk the controls.
* Android: Settings, Accessibility, TalkBack.
* Web: the page carries a mirror of every control; NVDA, VoiceOver or Orca
  read it.
* Linux: Orca, with `accessibility` enabled in the desktop settings.
