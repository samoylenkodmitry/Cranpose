# Screen identity and iOS text input validation

These checks started from main `2dd0372c` and were rebased onto `e37326ef` on 2026-09-22. The physical iPhone
ran CranScan with the local framework changes. Private receipt images, speech
transcripts and screenshots remain outside the repository and build cache.

## Shared identity

The allocation generation of a reused layout node was absent from the platform
identity. A regression with an unchanged node ID and a new generation failed:
the replacement retained platform ID 1. The test now passes for ordinary nodes
and canvas controls. Both layout-derived and retained semantics snapshots carry
the generation. Live regions, pane announcements, dialog detection and iOS
structure comparisons use the same identity.

On the iPhone, VoiceOver previously continued near the end of the receipt screen
after opening a library row. The corrected sequence begins at Back and reaches
Edit receipt through ordinary forward navigation and a double tap.

## Native text input

An inactive field previously announced its name without its text field role.
The adapter now obtains that role's traits from UIKit's native UITextField.
The physical iPhone returned the same traits with VoiceOver off and on. No
private trait constant is embedded in the framework.

The native input view is exposed only after it becomes the active responder.
Its frame follows the field's bounds. Reader focus stays on the inactive field
until activation. The native input view does not intercept ordinary touch input.

The phone test reached Merchant as a text field and activated it through
VoiceOver. Native evidence shows keyboard focus and the configured Gboard
keyboard. The app-scoped XCTest keyboard query does not identify this extension;
One run entered Q, dismissed the keyboard and saved the receipt. The next launch
loaded Q and VoiceOver announced it from the editor. The first spoken Q on the
device's Gboard keyboard was a suggestion before Question and Quick, followed
later by the letter keys. Selecting the existing Q suggestion did not change
either Cranpose's field or a native SwiftUI field. A native comparison reached
the spoken W key and changed Q to Qw. The CranScan test then passed the complete
sequence: open receipt, activate Merchant, append W through the spoken keyboard
key, hide the keyboard, save, reopen the editor, and hear its saved value. It
asserted the exact appended text before Save. This establishes that sequence on
the connected iPhone, not every text editing operation.

## Reader cursor and keyboard focus

Mobile reader navigation now reveals the control without changing keyboard
focus. A regression first reproduced an active editor changing from Merchant
to Payment when the reader cursor moved. Another assertion reproduced an
inactive field starting text input on reader navigation alone. Both now pass.

The scroll operation commits its state before layout. An extra surrounding
snapshot caused layout to read the previous scroll offset; the regression
failed with Payment still below the viewport and now passes before and after
the next frame. Tests cover both axes, reversed lists, hidden and disabled
targets, preserved selection, and subsequent manual scroll.

## Focused lazy items

The phone test exposed an active receipt editor leaving composition when reader
navigation scrolled to later content. A regression reproduced this through the
list's scroll-delta path. The list now measures the focused item's existing slot
before recycling other slots. It releases that item when focus is cleared or the
item is removed, and clears the removed item's text-input focus.

Retention alone left the editor absent from the placed accessibility tree. A
second regression reproduced that omission. The retained item now has a placement
outside the visible range, with distances based on measured item sizes and an
estimate for items that have not been measured. It retains its identity and can
be revealed through the reader action. Tests cover both axes, reversed layouts,
varied item sizes, off-screen text entry, focus release, and removal. Reader
reveal checks the new layout after a scroll and refines estimated distances only
while the target gets closer. An action that reports success without changing
geometry stops after one invocation. The physical edit-and-save flow passed with
the active field outside the visible area during keyboard navigation.

## Checks

| Check | Result |
| --- | --- |
| Framework library tests, desktop and wgpu features | 386 passed |
| UI library tests | 1,119 passed |
| Core library tests | 868 passed |
| App shell library tests | 256 passed |
| Platform scheduling contracts | 129 passed |
| Workspace lint | Passed |
| Shipped web lint recipe | Passed |
| Android lint recipe, all four targets | Passed |
| Windows cross-compilation lint recipe | Passed |
| iOS lint recipe | Passed |
| Published API documentation | Passed |
| Workspace documentation tests | Passed |
| Pre-commit checks | Passed |

The generation regression was observed to fail before its fix. Native tree
evidence is distinct from speech and complete user flows. These results do not
establish complete platform coverage or usability for blind people. See the
[platform validation protocol](accessibility_validation.md).
