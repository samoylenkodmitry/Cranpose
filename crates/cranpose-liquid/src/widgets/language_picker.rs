use cranpose_core::rememberMutableStateOf;
use cranpose_macros::composable;
use cranpose_ui::{
    LanguagePreference, LinearArrangement, LocalizationController, Modifier,
    text::TextStyle,
    widgets::{Column, ColumnSpec, FlowRow, FlowRowSpec, Text},
};

use super::{
    button::{GlassButton, GlassButtonLabel, GlassButtonSize, GlassButtonSpec},
    card::LiquidCard,
};
use crate::theme::{liquid_colors, liquid_typography};

/// A language chooser for an application that uses Cranpose localization.
///
/// The app's catalog supplies the offered languages and their native names.
/// The controller saves a selection, updates the app's translator, and exposes
/// any storage error for this widget to display. Enable the `localization`
/// feature on `cranpose-liquid` to use this component.
#[composable]
pub fn LiquidLanguagePicker(controller: LocalizationController) {
    let expanded = rememberMutableStateOf(|| false);
    let preference = controller.preference();
    let current_name = preference_name(&preference);
    let colors = liquid_colors();
    let typography = liquid_typography();
    let title = cranpose_ui::tr!("App language", id = "app-language");
    let change = cranpose_ui::tr!("Change app language", id = "change-language");
    let system_name = cranpose_ui::tr!("System default", id = "system-language");

    LiquidCard(Modifier::empty().fill_max_width(), move || {
        let preference = preference.clone();
        let current_name = current_name.clone();
        let title = title.clone();
        let change = change.clone();
        let system_name = system_name.clone();
        let controller = controller.clone();
        let expanded = expanded;
        let typography = typography.clone();
        let colors = colors;
        Column(
            Modifier::empty().padding(16.0),
            ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(8.0)),
            move || {
                let mut heading_style = typography.headline.clone();
                heading_style.span_style.color = Some(colors.label);
                Text(title.clone(), Modifier::empty(), heading_style);
                Text(
                    current_name.clone(),
                    Modifier::empty(),
                    typography.body.clone(),
                );

                let state_name = current_name.as_str().to_owned();
                let toggle_modifier =
                    Modifier::empty()
                        .fill_max_width()
                        .semantics(move |semantics| {
                            semantics.state_description = Some(state_name.clone());
                        });
                let toggle = expanded;
                let change_label = change.clone();
                let change_spec = GlassButtonSpec::glass();
                GlassButton(
                    toggle_modifier,
                    change_spec.clone(),
                    move || toggle.set(!toggle.get()),
                    move || GlassButtonLabel(change_label.clone(), change_spec.clone()),
                );

                if expanded.get() {
                    let preference = preference.clone();
                    let controller = controller.clone();
                    let system_name = system_name.clone();
                    FlowRow(
                        Modifier::empty().fill_max_width(),
                        FlowRowSpec::new()
                            .main_axis_spacing(8.0)
                            .cross_axis_spacing(8.0),
                        move || {
                            let system_selected = preference == LanguagePreference::System;
                            let system_controller = controller.clone();
                            let system_expanded = expanded;
                            add_language_choice(system_name.clone(), system_selected, move || {
                                system_controller.select(LanguagePreference::System);
                                system_expanded.set(false);
                            });

                            for language in controller.catalog().languages() {
                                let selected =
                                    preference == LanguagePreference::Selected(language.clone());
                                let choice = language.clone();
                                let label = language.name().to_owned().into();
                                let choice_controller = controller.clone();
                                let choice_expanded = expanded;
                                add_language_choice(label, selected, move || {
                                    choice_controller
                                        .select(LanguagePreference::Selected(choice.clone()));
                                    choice_expanded.set(false);
                                });
                            }
                        },
                    );
                }

                if let Some(error) = controller.error() {
                    let message = cranpose_ui::tr!(
                        "Could not save language: {error}",
                        error = error,
                        id = "m-bae8fe9c44aa9004"
                    );
                    let mut error_style: TextStyle = typography.body.clone();
                    error_style.span_style.color = Some(colors.destructive);
                    Text(message, Modifier::empty(), error_style);
                }
            },
        );
    });
}

fn preference_name(preference: &LanguagePreference) -> cranpose_ui::SharedText {
    match preference {
        LanguagePreference::System => cranpose_ui::tr!("System default", id = "system-language"),
        LanguagePreference::Selected(language) => language.name().to_owned().into(),
    }
}

fn add_language_choice(
    label: cranpose_ui::SharedText,
    selected: bool,
    on_click: impl Fn() + 'static,
) {
    let modifier =
        Modifier::empty().semantics(move |semantics| semantics.selected = Some(selected));
    let spec = GlassButtonSpec::plain().with_size(GlassButtonSize::Small);
    GlassButton(modifier, spec.clone(), on_click, move || {
        GlassButtonLabel(label.clone(), spec.clone());
    });
}
