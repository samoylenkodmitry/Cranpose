# Accessibility plan: what a blind person needs first

Written 2026-09-17. The order is the one the user set: the holes that make an
app unusable without sight come first, and system options come last. A box
is ticked when the code, its tests and its docs are on the branch.

## Step 1. Text a reader can edit

- [x] `set_selection` in the config, the tree node and `BasicTextField`
- [x] projection: the caret on the element; byte, UTF-16 and character helpers
- [x] AccessKit: text runs, a text selection, the set-text-selection action
- [x] Android: the selection on the wire, granularity actions, set selection, text and selection events
- [x] iOS: the focused field as the keyboard's `UITextInput` view, input delegate notices
- [x] Web: an input or text area as the mirror, `selectionchange`, an in-place patch
- [x] docs, parity row, static test
- [ ] commit on `a11y-text-edit`

## Step 2. Roles a reader names

- [ ] new roles: Link, SearchField, ProgressBar, ToggleButton, Alert, Toolbar, Menu, MenuItem, TabBar, List, ListItem
- [ ] each role on every platform: projection, Android code and class name, AccessKit role, ARIA role, iOS trait
- [ ] widgets declare them: progress indicators, `LiquidTabBar`, liquid menu rows, lazy lists
- [ ] docs, parity row, static test

## Step 3. What VoiceOver and Voice Control users expect

- [ ] `input_labels`, the short names Voice Control shows (`accessibilityUserInputLabels`)
- [ ] `on_magic_tap`: the iOS magic tap, a named action on the other platforms
- [ ] `language` per node on iOS, AccessKit and the web
- [ ] reader state: `local_accessibility_state()` with `screen_reader_on`, fed by iOS, Android and AccessKit
- [ ] docs, static test

## Step 4. Keyboard and switch access

- [ ] Enter and Space activate the focused control
- [ ] arrow keys move focus inside a selectable group
- [ ] a focus ring drawn on keyboard focus
- [ ] `Modifier::minimum_interactive_component_size()`
- [ ] docs, tests

## Step 5. Checks that run without a hand

- [ ] `assert_accessible` in cranpose-testing: names, duplicate names, target size, order, pane title, images
- [ ] the desktop demo screens under the audit
- [ ] Android Accessibility Test Framework in the instrumented test
- [ ] a robot command that prints the tree the way a reader speaks it
- [ ] docs

## Step 6. System options, last

- [ ] one `AccessibilityOptions` value in cranpose-services
- [ ] font scale on iOS, the web and the desktop
- [ ] reduce motion into cranpose-animation
- [ ] reduce transparency into cranpose-liquid
- [ ] increase contrast, bold text and invert colors into the theme and images
- [ ] docs

## Step 7. cranscan takes it up

- [ ] waits for a Cranpose release that carries the steps above
- [ ] search field role, magic tap on capture, reader guidance, a shutter cue
