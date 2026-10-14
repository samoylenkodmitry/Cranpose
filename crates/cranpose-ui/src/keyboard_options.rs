//! Compose's `KeyboardOptions` and keyboard actions: which software keyboard
//! a text field asks for, and what its action key does.

use std::{cell::Cell, rc::Rc};

/// The keys a software keyboard offers: Compose's `KeyboardType`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum KeyboardType {
    /// The platform's text keyboard.
    #[default]
    Text,
    /// Letters, digits and symbols from ASCII only.
    Ascii,
    /// Digits.
    Number,
    /// Digits and the symbols of a phone number.
    Phone,
    /// Text keys for a web address.
    Uri,
    /// Text keys for an email address.
    Email,
    /// Text keys for a password: no suggestions, no learning from it.
    Password,
    /// Digits for a PIN: no suggestions, no learning from it.
    NumberPassword,
    /// Digits and a decimal separator.
    Decimal,
}

impl KeyboardType {
    /// Whether the keyboard enters a secret, which it neither corrects nor
    /// learns from.
    pub fn is_password(self) -> bool {
        matches!(self, Self::Password | Self::NumberPassword)
    }
}

/// Which letters a software keyboard capitalizes on its own: Compose's
/// `KeyboardCapitalization`. Only text keyboards capitalize.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum KeyboardCapitalization {
    /// Capitalizes nothing.
    #[default]
    None,
    /// Every character.
    Characters,
    /// The first letter of every word.
    Words,
    /// The first letter of every sentence.
    Sentences,
}

/// The action key a software keyboard shows, which Enter also runs in a
/// single-line field: Compose's `ImeAction`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ImeAction {
    /// [`Done`](Self::Done) in a single-line field, [`None`](Self::None) in
    /// a multi-line one.
    #[default]
    Default,
    /// No action: Enter adds a line in a multi-line field and does nothing
    /// in a single-line one.
    None,
    /// Opens the destination the person typed, such as a web address.
    Go,
    /// Runs the search for the text typed.
    Search,
    /// Sends the text typed.
    Send,
    /// Moves focus to the previous field by default.
    Previous,
    /// Moves focus to the next field by default.
    Next,
    /// Hides the software keyboard by default.
    Done,
}

impl ImeAction {
    /// The action a field with these lines shows: [`Default`](Self::Default)
    /// resolved, any other action as it is.
    pub fn resolve(self, single_line: bool) -> Self {
        match self {
            Self::Default if single_line => Self::Done,
            Self::Default => Self::None,
            action => action,
        }
    }
}

/// What a text field holds, so autofill and password managers can offer to
/// fill it: Compose's `ContentType`. Android, iOS and web pass it to the
/// platform.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ContentType {
    /// A username or login name.
    Username,
    /// A password.
    Password,
    /// A username the person is choosing.
    NewUsername,
    /// A password the person is choosing, for which a password manager can
    /// suggest a strong one.
    NewPassword,
    /// An email address.
    EmailAddress,
    /// A phone number.
    PhoneNumber,
    /// A person's full name.
    PersonFullName,
    /// A street address.
    PostalAddress,
    /// A postal code.
    PostalCode,
    /// A credit card number.
    CreditCardNumber,
    /// A one-time code from a text message.
    SmsOtpCode,
}

/// The software keyboard a text field asks for: Compose's `KeyboardOptions`.
/// Platforms without a software keyboard ignore everything but the action,
/// which Enter runs in a single-line field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KeyboardOptions {
    /// The keys the keyboard offers.
    pub keyboard_type: KeyboardType,
    /// Which letters the keyboard capitalizes on its own.
    pub capitalization: KeyboardCapitalization,
    /// Whether the keyboard corrects and suggests words. Password keyboards
    /// never do.
    pub auto_correct: bool,
    /// The action key the keyboard shows. Enter runs the same action in a single-line field.
    pub ime_action: ImeAction,
    /// What the field holds, for autofill. Compose sets this through the
    /// field's semantics.
    pub content_type: Option<ContentType>,
}

impl Default for KeyboardOptions {
    fn default() -> Self {
        Self {
            keyboard_type: KeyboardType::Text,
            capitalization: KeyboardCapitalization::None,
            auto_correct: true,
            ime_action: ImeAction::Default,
            content_type: None,
        }
    }
}

impl KeyboardOptions {
    /// Whether the keyboard corrects and suggests words: asked for, and not
    /// a password keyboard.
    pub fn corrects(&self) -> bool {
        self.auto_correct && !self.keyboard_type.is_password()
    }
}

/// What a keyboard action handler gets: the action the person ran, and a
/// way to run that action's default behavior too, as Compose's
/// `performDefaultAction` does.
pub struct KeyboardActionScope {
    action: ImeAction,
    default_requested: Cell<bool>,
}

impl KeyboardActionScope {
    /// The resolved action: never [`ImeAction::Default`].
    pub fn action(&self) -> ImeAction {
        self.action
    }

    /// Runs the action's default after the handler returns: Next and
    /// Previous move focus, Done hides the software keyboard, and the other
    /// actions do nothing.
    pub fn perform_default_action(&self) {
        self.default_requested.set(true);
    }
}

/// Runs when the person presses a field's keyboard action key, or Enter in
/// a single-line field. A field without a handler runs the action's
/// default; a handler runs it only through
/// [`KeyboardActionScope::perform_default_action`].
#[derive(Clone)]
pub struct KeyboardActionHandler(Rc<dyn Fn(&KeyboardActionScope)>);

impl KeyboardActionHandler {
    /// Wraps `handler`, which runs on a keyboard action with a [`KeyboardActionScope`].
    pub fn new(handler: impl Fn(&KeyboardActionScope) + 'static) -> Self {
        Self(Rc::new(handler))
    }
}

impl std::fmt::Debug for KeyboardActionHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("KeyboardActionHandler")
    }
}

impl PartialEq for KeyboardActionHandler {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

/// Runs `action`, resolved, through `handler` or else its default.
pub(crate) fn run_keyboard_action(action: ImeAction, handler: Option<&KeyboardActionHandler>) {
    let scope = KeyboardActionScope {
        action,
        default_requested: Cell::new(handler.is_none()),
    };
    if let Some(handler) = handler {
        (handler.0)(&scope);
    }
    if scope.default_requested.get() {
        perform_default_action(action);
    }
}

fn perform_default_action(action: ImeAction) {
    match action {
        ImeAction::Next => crate::focus_order::request_focus_move(crate::FocusDirection::Next),
        ImeAction::Previous => {
            crate::focus_order::request_focus_move(crate::FocusDirection::Previous);
        }
        ImeAction::Done => crate::text_input_session::hide_software_keyboard(),
        ImeAction::Default
        | ImeAction::None
        | ImeAction::Go
        | ImeAction::Search
        | ImeAction::Send => {}
    }
}
