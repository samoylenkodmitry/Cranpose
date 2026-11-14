#![expect(unsafe_code, reason = "Objective-C messages to UIKit")]

use objc2::{MainThreadMarker, MainThreadOnly, rc::Retained, runtime::AnyObject, sel};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{NSArray, NSObject, NSString};
use objc2_ui_kit::{
    NSObjectUIAccessibility, NSObjectUIAccessibilityAction, UIAccessibilityCustomAction,
    UIAccessibilityTraitAdjustable, UIAccessibilityTraitButton, UIAccessibilityTraitHeader,
    UIAccessibilityTraitImage, UIAccessibilityTraitLink, UIAccessibilityTraitNone,
    UIAccessibilityTraitNotEnabled, UIAccessibilityTraitSearchField, UIAccessibilityTraitSelected,
    UIAccessibilityTraitStaticText, UIAccessibilityTraitUpdatesFrequently, UIAccessibilityTraits,
    UITextField,
};

use super::NativeAccessibilityElement;
use crate::accessibility::{self, AccessibilityElement, AccessibilityRole};

pub(super) fn update_native_element(
    native: &NativeAccessibilityElement,
    element: &AccessibilityElement,
    elements: &[AccessibilityElement],
    mtm: MainThreadMarker,
) {
    let jumpable = accessibility::scroll_container_for(elements, element)
        .is_some_and(|container| accessibility::row_count(container) > 0);
    let details = element.details();
    native.set_actionable(element.clickable || element.role.is_text_field());
    native.set_dismissable(details.dismissable);
    native.set_magic_tap(details.magic_tap_label.is_some());
    native.setAccessibilityLanguage(
        details
            .language
            .as_deref()
            .map(NSString::from_str)
            .as_deref(),
        mtm,
    );
    let input_labels: Vec<Retained<NSString>> = details
        .input_labels
        .iter()
        .map(|label| NSString::from_str(label))
        .collect();
    let input_labels =
        (!input_labels.is_empty()).then(|| NSArray::from_retained_slice(&input_labels));
    // SAFETY: the array holds retained strings and lives until the call
    // returns; UIKit copies what it keeps.
    unsafe {
        native.setAccessibilityUserInputLabels(input_labels.as_deref(), mtm);
    }
    native.setIsAccessibilityElement(!element.label.is_empty() || element.role.is_text_field());
    native.setAccessibilityLabel(Some(&NSString::from_str(&element.label)));
    let value = accessibility::voiceover_value(element);
    native.setAccessibilityValue(value.as_deref().map(NSString::from_str).as_deref());
    let hint = [details.click_label.as_deref(), details.hint_text.as_deref()]
        .into_iter()
        .flatten()
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    native.setAccessibilityHint(
        (!hint.is_empty())
            .then(|| NSString::from_str(&hint))
            .as_deref(),
    );
    native.setAccessibilityFrameInContainerSpace(CGRect::new(
        CGPoint::new(element.bounds.x as f64, element.bounds.y as f64),
        CGSize::new(element.bounds.width as f64, element.bounds.height as f64),
    ));
    let mut traits = role_traits(element.role, mtm);
    // SAFETY: UIKit accessibility trait constants are immutable process-wide
    // values exported by the linked framework.
    unsafe {
        if element.selected == Some(true) {
            traits |= UIAccessibilityTraitSelected;
        }
        if !element.enabled {
            traits |= UIAccessibilityTraitNotEnabled;
        }
        if details.adjustable {
            traits |= UIAccessibilityTraitAdjustable;
        }
    }
    native.setAccessibilityTraits(traits);
    native.setAccessibilityViewIsModal(details.is_modal, mtm);
    offer_custom_actions(native, element, jumpable, mtm);
}

fn text_field_traits(mtm: MainThreadMarker) -> UIAccessibilityTraits {
    crate::thread_state::with(|state| {
        *state.ios.text_field_traits.get_or_init(|| {
            let field = UITextField::new(mtm);
            let object: &NSObject = &field;
            object.accessibilityTraits(mtm)
        })
    })
}

fn role_traits(role: AccessibilityRole, mtm: MainThreadMarker) -> UIAccessibilityTraits {
    // SAFETY: UIKit accessibility trait constants are immutable process-wide
    // values exported by the linked framework.
    unsafe {
        match role {
            AccessibilityRole::Button
            | AccessibilityRole::Checkbox
            | AccessibilityRole::Switch
            | AccessibilityRole::RadioButton
            | AccessibilityRole::Tab
            | AccessibilityRole::DropdownList => UIAccessibilityTraitButton,
            AccessibilityRole::StaticText => UIAccessibilityTraitStaticText,
            AccessibilityRole::TextField => text_field_traits(mtm),
            AccessibilityRole::Image => UIAccessibilityTraitImage,
            AccessibilityRole::ValuePicker => UIAccessibilityTraitAdjustable,
            AccessibilityRole::Header | AccessibilityRole::Dialog => UIAccessibilityTraitHeader,
            AccessibilityRole::Link
            | AccessibilityRole::SearchField
            | AccessibilityRole::ProgressBar
            | AccessibilityRole::ToggleButton
            | AccessibilityRole::Alert
            | AccessibilityRole::Toolbar
            | AccessibilityRole::Menu
            | AccessibilityRole::MenuItem
            | AccessibilityRole::TabBar
            | AccessibilityRole::List
            | AccessibilityRole::ListItem
            | AccessibilityRole::RadioGroup => named_role_traits(role, mtm),
        }
    }
}

/// The VoiceOver traits of the roles beyond Compose's own. A toolbar, a
/// menu, a tab bar and a list carry no label, so they are never elements.
fn named_role_traits(role: AccessibilityRole, mtm: MainThreadMarker) -> UIAccessibilityTraits {
    // SAFETY: UIKit accessibility trait constants are immutable process-wide
    // values exported by the linked framework.
    unsafe {
        match role {
            AccessibilityRole::Link => UIAccessibilityTraitLink,
            AccessibilityRole::SearchField => UIAccessibilityTraitSearchField,
            AccessibilityRole::ProgressBar => UIAccessibilityTraitUpdatesFrequently,
            AccessibilityRole::ToggleButton | AccessibilityRole::MenuItem => {
                UIAccessibilityTraitButton
            }
            AccessibilityRole::Alert | AccessibilityRole::ListItem => {
                UIAccessibilityTraitStaticText
            }
            AccessibilityRole::Toolbar
            | AccessibilityRole::Menu
            | AccessibilityRole::TabBar
            | AccessibilityRole::List
            | AccessibilityRole::RadioGroup => UIAccessibilityTraitNone,
            AccessibilityRole::Button
            | AccessibilityRole::StaticText
            | AccessibilityRole::TextField
            | AccessibilityRole::Checkbox
            | AccessibilityRole::Switch
            | AccessibilityRole::RadioButton
            | AccessibilityRole::Tab
            | AccessibilityRole::Image
            | AccessibilityRole::DropdownList
            | AccessibilityRole::ValuePicker
            | AccessibilityRole::Header
            | AccessibilityRole::Dialog => role_traits(role, mtm),
        }
    }
}

/// Lists the actions the element offers in VoiceOver's actions rotor, each
/// one aimed back at the element by name, and after them the two ends of the
/// list the element sits in. VoiceOver has no long press of its own, so a
/// long press is the last of the named actions.
fn offer_custom_actions(
    native: &NativeAccessibilityElement,
    element: &AccessibilityElement,
    jumpable: bool,
    mtm: MainThreadMarker,
) {
    let labels = accessibility::reader_actions(element);
    native.set_custom_action_labels(&labels);
    let mut actions: Vec<Retained<UIAccessibilityCustomAction>> = labels
        .iter()
        .map(|label| rotor_action(native, label, sel!(performAccessibilityCustomAction:), mtm))
        .collect();
    if jumpable {
        actions.push(rotor_action(
            native,
            "first row",
            sel!(cranposeJumpToFirstRow:),
            mtm,
        ));
        actions.push(rotor_action(
            native,
            "last row",
            sel!(cranposeJumpToLastRow:),
            mtm,
        ));
    }
    let native_object: &NSObject = native;
    let list = (!actions.is_empty()).then(|| NSArray::from_retained_slice(&actions));
    native_object.setAccessibilityCustomActions(list.as_deref(), mtm);
}

/// One entry of VoiceOver's actions rotor, named for the user and aimed at a
/// method of the element under the cursor.
fn rotor_action(
    native: &NativeAccessibilityElement,
    label: &str,
    selector: objc2::runtime::Sel,
    mtm: MainThreadMarker,
) -> Retained<UIAccessibilityCustomAction> {
    let target: &AnyObject = native.as_ref();
    // SAFETY: the target is this element, which answers every selector passed
    // here and outlives the action.
    unsafe {
        UIAccessibilityCustomAction::initWithName_target_selector(
            UIAccessibilityCustomAction::alloc(mtm),
            &NSString::from_str(label),
            Some(target),
            selector,
        )
    }
}
