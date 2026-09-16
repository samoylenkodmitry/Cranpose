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

## What a reader hears, end to end

1. Layout builds the semantics tree, one node per control, with focus flags.
2. `crates/cranpose/src/accessibility.rs` projects that tree onto flat elements
   with screen bounds: one platform-neutral shape, four bridges.
3. Each bridge turns an element into the platform's own node, and turns the
   platform's actions back into a click, a custom action, or a focus move.

Nothing in that path is platform specific above the bridge, so a control that
reads correctly on one platform reads correctly on all four. The static test
`every_platform_bridge_reads_announcements_out` and its focus counterpart in
`crates/cranpose/tests/platform_scheduling_static.rs` keep the four bridges in
step.

## Check it by hand

* iOS: Settings, Accessibility, VoiceOver. Swipe right to walk the controls.
* Android: Settings, Accessibility, TalkBack.
* Web: the page carries a mirror of every control; NVDA, VoiceOver or Orca
  read it.
* Linux: Orca, with `accessibility` enabled in the desktop settings.
