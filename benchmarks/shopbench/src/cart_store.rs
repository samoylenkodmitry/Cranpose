// The cart, in memory only: the first add creates it, later adds and the
// stepper change its lines, and every answer replaces the cart shown.

use coroflow::MainScope;
use cranpose::MutableState;
use cranpose_core::{OwnedMutableState, ownedMutableStateOf};

use crate::{
    api::Api,
    model::{Cart, CartPayload},
};

const NETWORK_FAILURE: &str = "Couldn't update the cart. Check your connection and try again.";

pub(crate) struct CartStore {
    api: Api,
    scope: MainScope,
    cart: OwnedMutableState<Option<Cart>>,
    busy: OwnedMutableState<bool>,
    alert: OwnedMutableState<Option<String>>,
}

impl CartStore {
    pub(crate) fn new(scope: MainScope, api: Api) -> Self {
        Self {
            api,
            scope,
            cart: ownedMutableStateOf(None),
            busy: ownedMutableStateOf(false),
            alert: ownedMutableStateOf(None),
        }
    }

    pub(crate) fn cart(&self) -> MutableState<Option<Cart>> {
        self.cart.handle()
    }

    pub(crate) fn busy(&self) -> MutableState<bool> {
        self.busy.handle()
    }

    pub(crate) fn alert(&self) -> MutableState<Option<String>> {
        self.alert.handle()
    }

    // Adds one of `variant`, then tells `done` whether the line went in
    // without an error.
    pub(crate) fn add(&self, variant: String, done: impl FnOnce(bool) + 'static) {
        let (api, cart, busy, alert) = (self.api.clone(), self.cart(), self.busy(), self.alert());
        busy.set(true);
        self.scope.launch(async move {
            let id = cart.with(|cart| cart.as_ref().map(|cart| cart.id.clone()));
            let payload = match id {
                Some(id) => api.cart_lines_add(&id, &variant).await,
                None => api.cart_create(&variant).await,
            };
            busy.set(false);
            done(apply(payload, cart, alert));
        });
    }

    // Sets the quantity of `line`; 0 removes it.
    pub(crate) fn update(&self, line: String, quantity: u32) {
        let Some(id) = self
            .cart
            .handle()
            .with(|cart| cart.as_ref().map(|cart| cart.id.clone()))
        else {
            return;
        };
        let (api, cart, busy, alert) = (self.api.clone(), self.cart(), self.busy(), self.alert());
        busy.set(true);
        self.scope.launch(async move {
            let payload = api.cart_lines_update(&id, &line, quantity).await;
            busy.set(false);
            apply(payload, cart, alert);
        });
    }
}

// Shows the answer's cart and its first error; answers whether the change
// went in.
fn apply(
    payload: anyhow::Result<CartPayload>,
    cart: MutableState<Option<Cart>>,
    alert: MutableState<Option<String>>,
) -> bool {
    let Ok(payload) = payload else {
        alert.set(Some(NETWORK_FAILURE.to_owned()));
        return false;
    };
    let arrived = payload.cart.is_some();
    if let Some(answer) = payload.cart {
        cart.set(Some(answer));
    }
    if let Some(error) = payload.user_errors.into_iter().next() {
        alert.set(Some(error.message));
        return false;
    }
    arrived
}
