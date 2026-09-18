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
- [x] commit on `a11y-text-edit`

## Step 2. Roles a reader names

- [x] new roles: Link, SearchField, ProgressBar, ToggleButton, Alert, Toolbar, Menu, MenuItem, TabBar, List, ListItem
- [x] each role on every platform: projection, Android code and class name, AccessKit role, ARIA role, iOS trait
- [x] widgets declare them: progress indicators, `LiquidTabBar`, liquid menu rows, lazy lists
- [x] docs, parity row, static test

## Step 3. What VoiceOver and Voice Control users expect

- [x] `input_labels`, the short names Voice Control shows (`accessibilityUserInputLabels`)
- [x] `on_magic_tap`: the iOS magic tap, a named action on the other platforms
- [x] `language` per node on iOS, AccessKit and the web
- [x] reader state: `local_accessibility_state()` with `screen_reader_on`, fed by iOS, Android and AccessKit
- [x] docs, static test

## Step 4. Keyboard and switch access

- [x] Enter and Space activate the focused control
- [x] arrow keys move focus inside a selectable group, and inside a menu
- [x] a focus ring drawn on keyboard focus; every clickable control takes focus
- [x] `Modifier::minimum_interactive_component_size()`
- [x] docs, tests

## Step 5. Checks that run without a hand

- [x] `assert_accessible` in cranpose-testing: names, duplicate names, target size, order, pane title, images
- [x] the desktop demo screens under the audit, with the issues it found fixed and five left on a list that only shrinks
- [x] Android Accessibility Test Framework in the instrumented test
- [x] a robot command that prints the tree the way a reader speaks it: `robot.spoken_tree()`
- [x] docs

## Step 6. System options, last

- [x] one `AccessibilityOptions` value in cranpose-services
- [x] font scale on iOS, the web and the desktop
- [x] reduce motion into cranpose-animation
- [x] reduce transparency into cranpose-liquid
- [x] increase contrast, bold text and invert colors into the theme and images
- [x] docs

## Step 7. cranscan takes it up

- [x] the robot of every app can audit the screen it shows: `robot.audit_accessibility()` and `robot.assert_accessible()`, and `audit_changes` keeps the list of issues a suite leaves as they are
- [x] in cranscan, on branch `a11y/step-7` built against the Cranpose branch: the search field role from the framework, the magic tap on the shutter, "Photo taken" through the announcer, a live hint that names the shutter when a reader is on, and an audit of every screen the robot suite photographs plus About, Insights and Unlock
- [x] what that audit found and cranscan fixed: same named Edit item, Remove and Row actions controls, five unnamed switches, and six targets under 24 points
- [ ] a Cranpose release with the steps above and the robot audit, then cranscan takes that version and the branch merges

## Step 8. What the first audits taught

- [x] `width_in`, `height_in` and `size_in`: a small field gets its 24 points without growing to 48
- [x] rows of one list with one name are apart in the audit, because a reader speaks their place
- [ ] a decorated text field's semantics cover its decoration box, so the padded field is the target a reader and a finger get
- [ ] a touch slop: a press within the minimum target of a control that nothing else claims reaches it, the way Compose's minimum touch target hit testing does; the clickable node has to take a release inside the slop too
- [ ] a check by hand of the demo on the iOS simulator through its accessibility tree: names, roles, traits, the magic tap
- [ ] a check by hand of the web mirror in a browser: focus order and live regions
- [ ] cranscan on the iPhone with VoiceOver on
